# Windows x64 설치 프로그램

PowerShell 7과 NSIS 3을 사용합니다. 앱을 먼저 빌드하고, 배포용 Kiwi 런타임과 모델, Hunspell 사전, 제3자 라이선스를 준비합니다. 런타임 루트에는 `kiwi`, `hunspell`, `licenses` 폴더가 있어야 합니다. `VCRuntimeDll`에는 Visual Studio의 x64 재배포용 `vcruntime140.dll` 경로를 지정합니다. [Microsoft C++ 재배포 문서](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files)

```powershell
./installer/Build-Installer.ps1 -AppExe <빌드한-dtool.exe> `
    -RuntimeDirectory <런타임-루트> -OutputDirectory <비어-있는-배포-작업-폴더> `
    -VCRuntimeDll <vcruntime140.dll-경로> -MakeNsis <makensis.exe-경로>
```

스크립트는 Windows 설치 실행 파일과 SHA-256 파일을 만듭니다. 설치 프로그램은 현재 사용자 계정에 DTool을 설치하고, 시작 메뉴에 앱과 HTML 설명서 두 개의 바로가기를 만듭니다.
