; MahoRD Client Component Installer NSIS Script

!define PRODUCT_NAME "MahoRD Client"
!define PRODUCT_VERSION "0.1.0"
!define PRODUCT_PUBLISHER "MahoRD Contributors"
!define PRODUCT_WEB_SITE "https://github.com/mahord/mahord"
!define PRODUCT_DIR_REGKEY "Software\Microsoft\Windows\CurrentVersion\App Paths\maho-client.exe"
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
OutFile "maho-client-Setup-${PRODUCT_VERSION}.exe"
InstallDir "$PROGRAMFILES64\MahoRD"
InstallDirRegKey HKLM "${PRODUCT_DIR_REGKEY}" ""
ShowInstDetails show
ShowUnInstDetails show

Section "MainSection" SEC01
  SetOutPath "$INSTDIR"
  SetOverwrite try

  File /nonfatal "maho-client.exe"
  File /nonfatal "tauri-shell.exe"
  File /nonfatal "..\..\target\release\maho-client.exe"
  File /nonfatal "..\..\target\release\tauri-shell.exe"
  File /nonfatal "..\..\target\x86_64-pc-windows-msvc\release\maho-client.exe"
  File /nonfatal "..\..\target\x86_64-pc-windows-msvc\release\tauri-shell.exe"

  ; FFmpeg Dynamic Shared Libraries required for HEVC decoding
  File /nonfatal "avcodec-*.dll"
  File /nonfatal "avformat-*.dll"
  File /nonfatal "avutil-*.dll"
  File /nonfatal "swscale-*.dll"
  File /nonfatal "swresample-*.dll"
  File /nonfatal "README-FFMPEG.md"

  CreateDirectory "$SMPROGRAMS\MahoRD"
  CreateShortcut "$SMPROGRAMS\MahoRD\MahoRD Client.lnk" "$INSTDIR\maho-client.exe"
  CreateShortcut "$DESKTOP\MahoRD Client.lnk" "$INSTDIR\maho-client.exe"
  CreateShortcut "$SMPROGRAMS\MahoRD\Uninstall Client.lnk" "$INSTDIR\uninstall-client.exe"
SectionEnd

Section -Post
  WriteUninstaller "$INSTDIR\uninstall-client.exe"
  WriteRegStr HKLM "${PRODUCT_DIR_REGKEY}" "" "$INSTDIR\maho-client.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayName" "$(^Name)"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "UninstallString" "$INSTDIR\uninstall-client.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayIcon" "$INSTDIR\maho-client.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "URLInfoAbout" "${PRODUCT_WEB_SITE}"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "Publisher" "${PRODUCT_PUBLISHER}"
SectionEnd

Section Uninstall
  Delete "$INSTDIR\uninstall-client.exe"
  Delete "$INSTDIR\maho-client.exe"
  Delete "$INSTDIR\tauri-shell.exe"
  Delete "$INSTDIR\README-FFMPEG.md"
  Delete "$INSTDIR\avcodec-*.dll"
  Delete "$INSTDIR\avformat-*.dll"
  Delete "$INSTDIR\avutil-*.dll"
  Delete "$INSTDIR\swscale-*.dll"
  Delete "$INSTDIR\swresample-*.dll"

  Delete "$DESKTOP\MahoRD Client.lnk"
  Delete "$SMPROGRAMS\MahoRD\MahoRD Client.lnk"
  Delete "$SMPROGRAMS\MahoRD\Uninstall Client.lnk"

  RMDir "$SMPROGRAMS\MahoRD"
  RMDir "$INSTDIR"

  DeleteRegKey ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}"
  DeleteRegKey HKLM "${PRODUCT_DIR_REGKEY}"
  SetAutoClose true
SectionEnd
