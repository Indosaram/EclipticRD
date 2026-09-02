; EclipticRD Client Component Installer NSIS Script

!define PRODUCT_NAME "EclipticRD Client"
!define PRODUCT_VERSION "0.1.0"
!define PRODUCT_PUBLISHER "EclipticRD Contributors"
!define PRODUCT_WEB_SITE "https://github.com/eclipticrd/eclipticrd"
!define PRODUCT_DIR_REGKEY "Software\Microsoft\Windows\CurrentVersion\App Paths\erd-client.exe"
!define PRODUCT_UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}"
!define PRODUCT_UNINST_ROOT_KEY "HKLM"

SetCompressor /SOLID lzma
RequestExecutionLevel admin

!include "MUI2.nsh"

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"

Name "${PRODUCT_NAME} ${PRODUCT_VERSION}"
OutFile "erd-client-Setup-${PRODUCT_VERSION}.exe"
InstallDir "$PROGRAMFILES64\EclipticRD"
InstallDirRegKey HKLM "${PRODUCT_DIR_REGKEY}" ""
ShowInstDetails show
ShowUnInstDetails show

Section "MainSection" SEC01
  SetOutPath "$INSTDIR"
  SetOverwrite try

  File /nonfatal "erd-client.exe"
  File /nonfatal "tauri-shell.exe"
  File /nonfatal "..\..\target\release\erd-client.exe"
  File /nonfatal "..\..\target\release\tauri-shell.exe"
  File /nonfatal "..\..\target\x86_64-pc-windows-msvc\release\erd-client.exe"
  File /nonfatal "..\..\target\x86_64-pc-windows-msvc\release\tauri-shell.exe"

  ; FFmpeg Dynamic Shared Libraries required for HEVC decoding
  File /nonfatal "avcodec-*.dll"
  File /nonfatal "avformat-*.dll"
  File /nonfatal "avutil-*.dll"
  File /nonfatal "swscale-*.dll"
  File /nonfatal "swresample-*.dll"
  File /nonfatal "README-FFMPEG.md"

  CreateDirectory "$SMPROGRAMS\EclipticRD"
  CreateShortcut "$SMPROGRAMS\EclipticRD\EclipticRD Client.lnk" "$INSTDIR\erd-client.exe"
  CreateShortcut "$DESKTOP\EclipticRD Client.lnk" "$INSTDIR\erd-client.exe"
  CreateShortcut "$SMPROGRAMS\EclipticRD\Uninstall Client.lnk" "$INSTDIR\uninstall-client.exe"
SectionEnd

Section -Post
  WriteUninstaller "$INSTDIR\uninstall-client.exe"
  WriteRegStr HKLM "${PRODUCT_DIR_REGKEY}" "" "$INSTDIR\erd-client.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayName" "$(^Name)"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "UninstallString" "$INSTDIR\uninstall-client.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayIcon" "$INSTDIR\erd-client.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "URLInfoAbout" "${PRODUCT_WEB_SITE}"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "Publisher" "${PRODUCT_PUBLISHER}"
SectionEnd

Section Uninstall
  Delete "$INSTDIR\uninstall-client.exe"
  Delete "$INSTDIR\erd-client.exe"
  Delete "$INSTDIR\tauri-shell.exe"
  Delete "$INSTDIR\README-FFMPEG.md"
  Delete "$INSTDIR\avcodec-*.dll"
  Delete "$INSTDIR\avformat-*.dll"
  Delete "$INSTDIR\avutil-*.dll"
  Delete "$INSTDIR\swscale-*.dll"
  Delete "$INSTDIR\swresample-*.dll"

  Delete "$DESKTOP\EclipticRD Client.lnk"
  Delete "$SMPROGRAMS\EclipticRD\EclipticRD Client.lnk"
  Delete "$SMPROGRAMS\EclipticRD\Uninstall Client.lnk"

  RMDir "$SMPROGRAMS\EclipticRD"
  RMDir "$INSTDIR"

  DeleteRegKey ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}"
  DeleteRegKey HKLM "${PRODUCT_DIR_REGKEY}"
  SetAutoClose true
SectionEnd
