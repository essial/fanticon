; Fanticon Windows installer script (NSIS + Modern UI 2).
;
; Built by CI with:
;   makensis /DVERSION=1.2.3 packaging\windows\fanticon.nsi
;
; Expects a "nsis-payload" directory at the repository root (created by the
; release workflow) containing:
;   fanticon-app.exe
;   README.md
;   demos\...
;   branding\...
;
; NSIS resolves File/Icon source paths relative to this .nsi script's own
; directory (packaging\windows), not the working directory makensis was
; invoked from -- so every relative path below walks back up to the repo
; root with "..\..\" first.

!ifndef VERSION
  !define VERSION "0.0.0"
!endif

Name "Fanticon"
OutFile "Fanticon-Setup-${VERSION}.exe"
InstallDir "$PROGRAMFILES64\Fanticon"
InstallDirRegKey HKLM "Software\Fanticon" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma

!include "MUI2.nsh"

!define MUI_ICON "..\..\assets\branding\fanticon.ico"
!define MUI_UNICON "..\..\assets\branding\fanticon.ico"
!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Fanticon" SEC_APP
  SetOutPath "$INSTDIR"
  File "..\..\nsis-payload\fanticon-app.exe"
  File "..\..\nsis-payload\README.md"
  SetOutPath "$INSTDIR\demos"
  File /r "..\..\nsis-payload\demos\*.*"
  SetOutPath "$INSTDIR\branding"
  File /r "..\..\nsis-payload\branding\*.*"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKLM "Software\Fanticon" "InstallDir" "$INSTDIR"

  CreateDirectory "$SMPROGRAMS\Fanticon"
  CreateShortcut "$SMPROGRAMS\Fanticon\Fanticon.lnk" "$INSTDIR\fanticon-app.exe"
  CreateShortcut "$SMPROGRAMS\Fanticon\Uninstall.lnk" "$INSTDIR\Uninstall.exe"

  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Fanticon" "DisplayName" "Fanticon"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Fanticon" "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Fanticon" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Fanticon" "Publisher" "Fanticon"
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Fanticon" "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Fanticon" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\fanticon-app.exe"
  Delete "$INSTDIR\README.md"
  RMDir /r "$INSTDIR\demos"
  RMDir /r "$INSTDIR\branding"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\Fanticon\Fanticon.lnk"
  Delete "$SMPROGRAMS\Fanticon\Uninstall.lnk"
  RMDir "$SMPROGRAMS\Fanticon"

  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Fanticon"
  DeleteRegKey HKLM "Software\Fanticon"
SectionEnd
