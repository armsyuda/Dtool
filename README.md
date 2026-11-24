# DTool 0.1

DTool은 벡터 작업과 페이지 레이아웃을 한 문서에서 다루는 Windows 디자인 앱입니다. 0.1은 첫 공개 버전입니다.

- [Windows x64 설치 파일](https://github.com/armsyuda/Dtool/releases/latest)
- [한국어 사용 설명서](docs/User-Guide.md)
- [한국어 MCP 연결 가이드](docs/MCP-Guide.md)

## 소스 빌드

Windows x64에서 `rust-toolchain.toml`의 Rust 도구 체인과 MSVC C++ Build Tools, Windows SDK를 준비합니다. 저장소 루트에서 아래 명령을 실행합니다.

```powershell
cargo build --locked --profile validation -p dtool-app --target-dir <빌드_폴더>
```

배포 패키지 조립 방법은 [Windows 설치 프로그램](installer/README.md)에 있습니다.
