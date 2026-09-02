; EclipticRD Host Component Installer NSIS Script

!define PRODUCT_NAME "EclipticRD Host"
!define PRODUCT_VERSION "0.1.0"
!define PRODUCT_PUBLISHER "EclipticRD Contributors"
!define PRODUCT_WEB_SITE "https://github.com/eclipticrd/eclipticrd"
!define PRODUCT_DIR_REGKEY "Software\Microsoft\Windows\CurrentVersion\App Paths\erd-host.exe"
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
OutFile "erd-host-Setup-${PRODUCT_VERSION}.exe"
InstallDir "$PROGRAMFILES64\EclipticRD"
InstallDirRegKey HKLM "${PRODUCT_DIR_REGKEY}" ""
ShowInstDetails show
ShowUnInstDetails show

Section "MainSection" SEC01
  SetOutPath "$INSTDIR"
  SetOverwrite try

  File /nonfatal "erd-host.exe"
  File /nonfatal "..\..\target\release\erd-host.exe"
  File /nonfatal "..\..\target\x86_64-pc-windows-msvc\release\erd-host.exe"

  ; Optional FFmpeg DLLs alongside host binary
  File /nonfatal "avcodec-*.dll"
  File /nonfatal "avformat-*.dll"
  File /nonfatal "avutil-*.dll"
  File /nonfatal "swscale-*.dll"
  File /nonfatal "swresample-*.dll"
  File /nonfatal "README-FFMPEG.md"

  CreateDirectory "$SMPROGRAMS\EclipticRD"
  CreateShortcut "$SMPROGRAMS\EclipticRD\EclipticRD Host.lnk" "$INSTDIR\erd-host.exe"
  CreateShortcut "$SMPROGRAMS\EclipticRD\Uninstall Host.lnk" "$INSTDIR\uninstall-host.exe"
SectionEnd

Section -Post
  WriteUninstaller "$INSTDIR\uninstall-host.exe"
  WriteRegStr HKLM "${PRODUCT_DIR_REGKEY}" "" "$INSTDIR\erd-host.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayName" "$(^Name)"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "UninstallString" "$INSTDIR\uninstall-host.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayIcon" "$INSTDIR\erd-host.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "URLInfoAbout" "${PRODUCT_WEB_SITE}"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "Publisher" "${PRODUCT_PUBLISHER}"
SectionEnd

Section Uninstall
  Delete "$INSTDIR\uninstall-host.exe"
  Delete "$INSTDIR\erd-host.exe"
  Delete "$INSTDIR\README-FFMPEG.md"
  Delete "$SMPROGRAMS\EclipticRD\EclipticRD Host.lnk"
  Delete "$SMPROGRAMS\EclipticRD\Uninstall Host.lnk"

  RMDir "$SMPROGRAMS\EclipticRD"
  RMDir "$INSTDIR"

  DeleteRegKey ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}"
  DeleteRegKey HKLM "${PRODUCT_DIR_REGKEY}"
  SetAutoClose true
SectionEnd
