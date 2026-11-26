use super::{ExportError, ExportOptions, RgbaImage, io_error, target_size};
use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
use windows::Graphics::Imaging::BitmapEncoder;
use windows::Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};

struct WinRtApartment;

impl WinRtApartment {
    fn enter() -> windows::core::Result<Self> {
        // The caller is a fresh worker thread with no apartment.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }?;
        Ok(Self)
    }
}

impl Drop for WinRtApartment {
    fn drop(&mut self) {
        // Balance initialization on this worker after its WinRT objects are dropped.
        unsafe { RoUninitialize() };
    }
}

pub(super) fn render_page(
    pdf: &[u8],
    page_index: u32,
    dpi: f32,
    options: &ExportOptions,
) -> Result<RgbaImage, ExportError> {
    // Windows.Data.Pdf skips function-based shadings, including conical fills
    // and strokes. Interpret the same exported PDF with a renderer that handles them.
    if let Ok(document) = hayro::hayro_syntax::Pdf::new(pdf.to_vec())
        && document.objects().into_iter().any(|object| {
            object.into_dict().and_then(|dict| {
                dict.get::<hayro::hayro_syntax::object::Number>(b"ShadingType")
            }).is_some_and(|kind| kind.as_i64() == 1)
        })
    {
        return render_function_shading_page(&document, page_index, dpi, options);
    }
    // The GUI thread may already have a single-threaded COM apartment.
    std::thread::scope(|scope| {
        scope
            .spawn(|| render_in_apartment(pdf, page_index, dpi, options))
            .join()
            .map_err(|_| ExportError::Io("PDF preview worker failed".into()))?
    })
}

fn render_function_shading_page(
    document: &hayro::hayro_syntax::Pdf,
    page_index: u32,
    dpi: f32,
    options: &ExportOptions,
) -> Result<RgbaImage, ExportError> {
    use hayro::{RenderCache, RenderSettings, hayro_interpret::InterpreterSettings, kurbo::Affine};
    use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit, color::palette::css::WHITE};

    let page = document.pages().get(page_index as usize).ok_or_else(|| ExportError::InvalidArea("pages".into()))?;
    let (page_width, page_height) = page.render_dimensions();
    if !(page_width > 0.0 && page_height > 0.0 && page_width.is_finite() && page_height.is_finite()) {
        return Err(ExportError::InvalidArea("pages".into()));
    }
    let (width, height) = preview_size(page_width, page_height, dpi, options)?;
    let mut context = RenderContext::new(width as u16, height as u16);
    let scale = Affine::scale_non_uniform(f64::from(width) / f64::from(page_width), f64::from(height) / f64::from(page_height));
    hayro::render_into(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &mut context,
        scale * Affine::new(page.initial_transform(true).as_coeffs()),
    );
    context.flush();
    let mut pixels = Pixmap::new(width as u16, height as u16);
    context.render_with(&mut pixels, &mut Resources::default(), RasterizerSettings {
        target_init: TargetInit::Clear(WHITE),
        ..Default::default()
    });
    RgbaImage::from_raw(width, height, pixels.data_as_u8_slice().to_vec())
        .ok_or_else(|| ExportError::Io("PDF preview pixel buffer has an invalid size".into()))
}

fn preview_size(width_pt: f32, height_pt: f32, dpi: f32, options: &ExportOptions) -> Result<(u32, u32), ExportError> {
    let dpi = if dpi.is_finite() && dpi > 0.0 { dpi } else { 72.0 };
    target_size((0.0, 0.0, width_pt * dpi / 72.0, height_pt * dpi / 72.0), dpi, options)
}

fn render_in_apartment(
    pdf: &[u8],
    page_index: u32,
    dpi: f32,
    options: &ExportOptions,
) -> Result<RgbaImage, ExportError> {
    let _apartment = WinRtApartment::enter().map_err(io_error)?;
    let source = InMemoryRandomAccessStream::new().map_err(io_error)?;
    let writer = DataWriter::CreateDataWriter(&source).map_err(io_error)?;
    writer.WriteBytes(pdf).map_err(io_error)?;
    writer.StoreAsync().and_then(|operation| operation.join()).map_err(io_error)?;
    writer.DetachStream().map_err(io_error)?;
    source.Seek(0).map_err(io_error)?;
    let document = PdfDocument::LoadFromStreamAsync(&source)
        .and_then(|operation| operation.join())
        .map_err(io_error)?;
    let page = document.GetPage(page_index).map_err(io_error)?;
    let size = page.Size().map_err(io_error)?;
    let (width, height) = preview_size(size.Width * 72.0 / 96.0, size.Height * 72.0 / 96.0, dpi, options)?;
    let render = PdfPageRenderOptions::new().map_err(io_error)?;
    render.SetDestinationWidth(width).map_err(io_error)?;
    render.SetDestinationHeight(height).map_err(io_error)?;
    render.SetBitmapEncoderId(BitmapEncoder::PngEncoderId().map_err(io_error)?).map_err(io_error)?;

    let output = InMemoryRandomAccessStream::new().map_err(io_error)?;
    page.RenderWithOptionsToStreamAsync(&output, &render)
        .and_then(|operation| operation.join())
        .map_err(io_error)?;
    output.Seek(0).map_err(io_error)?;
    let reader = DataReader::CreateDataReader(&output).map_err(io_error)?;
    let length = u32::try_from(output.Size().map_err(io_error)?).map_err(io_error)?;
    let loaded = reader.LoadAsync(length).and_then(|operation| operation.join()).map_err(io_error)?;
    let mut png = vec![0; loaded as usize];
    reader.ReadBytes(&mut png).map_err(io_error)?;
    super::decode(&png, crate::ExportFormat::Png)
}
