Unicode true
!include "MUI2.nsh"
!include "x64.nsh"
!include "FileFunc.nsh"

Var UpdateParent

Name "DTool"
OutFile "${SETUP_EXE}"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\DTool"
InstallDir "$LOCALAPPDATA\Programs\DTool"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor lzma
VIProductVersion "${DTOOL_VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "DTool"
VIAddVersionKey /LANG=1033 "FileDescription" "DTool Windows Installer"
VIAddVersionKey /LANG=1033 "FileVersion" "${DTOOL_VERSION}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${DTOOL_VERSION}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Korean"
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "DTool requires 64-bit Windows."
    Abort
  ${EndIf}
  ${GetParameters} $0
  ${GetOptions} $0 "/DTOOLUPDATE=" $UpdateParent
  ClearErrors
  ${If} $UpdateParent != ""
    ; The main process starts setup only after all documents have been reviewed.
    System::Call 'kernel32::OpenProcess(i 0x00100000, i 0, i $UpdateParent) p.r1'
    ${If} $1 != 0
      System::Call 'kernel32::WaitForSingleObject(p r1, i 30000) i.r2'
      System::Call 'kernel32::CloseHandle(p r1)'
      ${If} $2 != 0
        SetErrorLevel 2
        Abort
      ${EndIf}
    ${EndIf}
  ${EndIf}
FunctionEnd

Function .onInstSuccess
  ${If} $UpdateParent != ""
    Exec '"$INSTDIR\dtool.exe"'
  ${EndIf}
FunctionEnd

Section "DTool"
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File /r "${PAYLOAD}\*"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortcut "$DESKTOP\DTool.lnk" "$INSTDIR\dtool.exe" "" "$INSTDIR\DTool.ico"
  CreateDirectory "$SMPROGRAMS\DTool"
  CreateShortcut "$SMPROGRAMS\DTool\DTool.lnk" "$INSTDIR\dtool.exe" "" "$INSTDIR\DTool.ico"
  CreateShortcut "$SMPROGRAMS\DTool\DTool User Guide.lnk" "$INSTDIR\docs\User-Guide.html"
  CreateShortcut "$SMPROGRAMS\DTool\DTool MCP Guide.lnk" "$INSTDIR\docs\MCP-Guide.html"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "DTool"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" '"$INSTDIR\DTool.ico"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${DTOOL_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  Delete "$DESKTOP\DTool.lnk"
  Delete "$SMPROGRAMS\DTool\DTool.lnk"
  Delete "$SMPROGRAMS\DTool\DTool User Guide.lnk"
  Delete "$SMPROGRAMS\DTool\DTool MCP Guide.lnk"
  RMDir "$SMPROGRAMS\DTool"
  !include "${UNINSTALL_FILES}"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
