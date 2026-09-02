; EclipticRD Full Suite Installer NSIS Script
; Packages both Host and Client binaries with optional FFmpeg runtime DLLs

!define PRODUCT_NAME "EclipticRD"
!define PRODUCT_VERSION "0.1.0"
!define PRODUCT_PUBLISHER "EclipticRD Contributors"
!define PRODUCT_WEB_SITE "https://github.com/eclipticrd/eclipticrd"
!define PRODUCT_DIR_REGKEY "Software\Microsoft\Windows\CurrentVersion\App Paths\erd-host.exe"
!define PRODUCT_UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}"
!define PRODUCT_UNINST_ROOT_KEY "HKLM"

SetCompressor /SOLID lzma

; Request administrator privileges for Program Files installation
RequestExecutionLevel admin

; Modern UI
!include "MUI2.nsh"
!include "LogicLib.nsh"

; Interface Settings
!define MUI_ABORTWARNING
!define MUI_ICON "${NSISDIR}\Contrib\Graphics\Icons\modern-install.ico"
!define MUI_UNICON "${NSISDIR}\Contrib\Graphics\Icons\modern-uninstall.ico"

; UI Pages
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\..\..\LICENSE"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

; Uninstaller Pages
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

; Languages
!insertmacro MUI_LANGUAGE "English"

Name "${PRODUCT_NAME} ${PRODUCT_VERSION}"
OutFile "EclipticRD-Setup-${PRODUCT_VERSION}.exe"
InstallDir "$PROGRAMFILES64\EclipticRD"
InstallDirRegKey HKLM "${PRODUCT_DIR_REGKEY}" ""
ShowInstDetails show
ShowUnInstDetails show

Section "Host Component (erd-host)" SEC_HOST
  SectionIn RO
  SetOutPath "$INSTDIR"
  SetOverwrite try

  ; Install Host Executable
  File /nonfatal "erd-host.exe"
  File /nonfatal "..\..\target\release\erd-host.exe"
  File /nonfatal "..\..\target\x86_64-pc-windows-msvc\release\erd-host.exe"

  ; Create shortcuts
  CreateDirectory "$SMPROGRAMS\EclipticRD"
  CreateShortcut "$SMPROGRAMS\EclipticRD\EclipticRD Host.lnk" "$INSTDIR\erd-host.exe"
SectionEnd

Section "Client Component (erd-client)" SEC_CLIENT
  SetOutPath "$INSTDIR"
  SetOverwrite try

  ; Install Client Executable (if available)
  File /nonfatal "erd-client.exe"
  File /nonfatal "tauri-shell.exe"
  File /nonfatal "..\..\target\release\erd-client.exe"
  File /nonfatal "..\..\target\release\tauri-shell.exe"
  File /nonfatal "..\..\target\x86_64-pc-windows-msvc\release\erd-client.exe"
  File /nonfatal "..\..\target\x86_64-pc-windows-msvc\release\tauri-shell.exe"

  CreateShortcut "$SMPROGRAMS\EclipticRD\EclipticRD Client.lnk" "$INSTDIR\erd-client.exe"
  CreateShortcut "$DESKTOP\EclipticRD Client.lnk" "$INSTDIR\erd-client.exe"
SectionEnd

Section "FFmpeg Runtime DLLs" SEC_FFMPEG
  SetOutPath "$INSTDIR"
  SetOverwrite try

  ; FFmpeg Dynamic Shared Libraries
  File /nonfatal "avcodec-*.dll"
  File /nonfatal "avformat-*.dll"
  File /nonfatal "avutil-*.dll"
  File /nonfatal "swscale-*.dll"
  File /nonfatal "swresample-*.dll"
  File /nonfatal "README-FFMPEG.md"
SectionEnd

Section -AdditionalIcons
  WriteIniStr "$INSTDIR\${PRODUCT_NAME}.url" "InternetShortcut" "URL" "${PRODUCT_WEB_SITE}"
  CreateShortcut "$SMPROGRAMS\EclipticRD\Website.lnk" "$INSTDIR\${PRODUCT_NAME}.url"
  CreateShortcut "$SMPROGRAMS\EclipticRD\Uninstall.lnk" "$INSTDIR\uninstall.exe"
SectionEnd

Section -Post
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "${PRODUCT_DIR_REGKEY}" "" "$INSTDIR\erd-host.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayName" "$(^Name)"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayIcon" "$INSTDIR\erd-host.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "URLInfoAbout" "${PRODUCT_WEB_SITE}"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "Publisher" "${PRODUCT_PUBLISHER}"
SectionEnd

Function un.onUninstSuccess
  HideWindow
  MessageBox MB_ICONINFORMATION|MB_OK "$(^Name) was successfully removed from your computer."
FunctionEnd

Function un.onInit
  MessageBox MB_ICONQUESTION|MB_YESNO|MB_DEFBUTTON2 "Are you sure you want to completely remove $(^Name) and all of its components?" IDYES +2
  Abort
FunctionEnd

Section Uninstall
  Delete "$INSTDIR\${PRODUCT_NAME}.url"
  Delete "$INSTDIR\uninstall.exe"
  Delete "$INSTDIR\erd-host.exe"
  Delete "$INSTDIR\erd-client.exe"
  Delete "$INSTDIR\tauri-shell.exe"
  Delete "$INSTDIR\README-FFMPEG.md"
  Delete "$INSTDIR\avcodec-*.dll"
  Delete "$INSTDIR\avformat-*.dll"
  Delete "$INSTDIR\avutil-*.dll"
  Delete "$INSTDIR\swscale-*.dll"
  Delete "$INSTDIR\swresample-*.dll"

  Delete "$SMPROGRAMS\EclipticRD\Uninstall.lnk"
  Delete "$SMPROGRAMS\EclipticRD\Website.lnk"
  Delete "$DESKTOP\EclipticRD Client.lnk"
  Delete "$SMPROGRAMS\EclipticRD\EclipticRD Host.lnk"
  Delete "$SMPROGRAMS\EclipticRD\EclipticRD Client.lnk"
  RMDir "$SMPROGRAMS\EclipticRD"

  RMDir "$INSTDIR"

  DeleteRegKey ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}"
  DeleteRegKey HKLM "${PRODUCT_DIR_REGKEY}"
  SetAutoClose true
SectionEnd
