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
    // The GUI thread may already have a single-threaded COM apartment.
    std::thread::scope(|scope| {
        scope
            .spawn(|| render_in_apartment(pdf, page_index, dpi, options))
            .join()
            .map_err(|_| ExportError::Io("PDF preview worker failed".into()))?
    })
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
    let dpi = if dpi.is_finite() && dpi > 0.0 { dpi } else { 72.0 };
    let (width, height) = target_size(
        (0.0, 0.0, size.Width * dpi / 96.0, size.Height * dpi / 96.0),
        dpi,
        options,
    )?;
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
