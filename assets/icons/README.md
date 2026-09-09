# EclipticRD Cross-Platform Logo & App Icon Assets

Unified cross-platform icon pack designed for **iOS**, **macOS**, **Windows**, **Linux**, **Web**, and **Tauri v2**.
Derived with mathematical precision from the reference orbital device sync design.

---

## 🎨 Color Palette & Specifications

| Element | Hex Code | RGB | Role |
| :--- | :--- | :--- | :--- |
| **Orbital Ring / Nodes** | `#FD6758` | `rgb(253, 103, 88)` | Sync Loop, Delta Arrow, Orbiting Planet Node |
| **Device Strokes** | `#1E242E` | `rgb(30, 36, 46)` | Dark phone outline, desktop monitor and stand |
| **Background** | `#E9EEF3` | `rgb(233, 238, 243)` | Solid light background |

---

## 📁 Directory Structure

```
assets/icons/
├── master/                          # Master Vector Sources
│   ├── eclipticrd-symbol.svg        # Pure vector symbol (transparent bg)
│   ├── eclipticrd-appicon.svg       # Master squircle icon (1024x1024)
│   ├── eclipticrd-ios.svg           # iOS full-bleed square master (1024x1024)
│   └── eclipticrd-macos.svg         # macOS squircle + drop shadow (1024x1024)
│
├── macos/                           # Apple macOS Assets
│   ├── icon.icns                    # Multi-resolution ICNS (16px up to 1024px Retina)
│   └── AppIcon.iconset/             # Standard 10-file iconset (@1x and @2x)
│
├── ios/                             # Apple iOS Assets
│   ├── AppIcon-1024.png             # 1024x1024 App Store master (24-bit RGB, no alpha)
│   └── AppIcon.appiconset/          # Xcode Asset Catalog (15 PNGs + Contents.json)
│
├── windows/                         # Microsoft Windows Assets
│   ├── icon.ico                     # Multi-res ICO (16, 24, 32, 48, 64, 128, 256 px)
│   ├── Square44x44Logo.png          # Start Menu / Taskbar tile
│   ├── Square71x71Logo.png          # Small tile
│   ├── Square150x150Logo.png        # Medium tile
│   ├── Square310x310Logo.png        # Large tile
│   └── StoreLogo.png                # Windows Store icon (50x50)
│
├── linux/                           # Linux Freedesktop Assets
│   ├── eclipticrd.desktop           # Freedesktop application entry
│   └── hicolor/                     # Standard XDG icon theme hierarchy
│       ├── scalable/apps/           # Scalable vector (eclipticrd.svg)
│       ├── symbolic/apps/           # Monochrome/symbolic vector (eclipticrd-symbolic.svg)
│       └── <size>x<size>/apps/      # 16, 24, 32, 48, 64, 96, 128, 256, 512, 1024 PNGs
│
├── web/                             # Web & PWA Assets
│   ├── favicon.ico                  # Multi-res browser favicon (16, 32, 48)
│   ├── favicon-16x16.png            # 16px favicon
│   ├── favicon-32x32.png            # 32px favicon
│   ├── apple-touch-icon.png         # 180x180 iOS Safari bookmark icon
│   ├── logo-symbol.svg              # Scalable branding symbol
│   └── logo-symbol-512.png          # High-res transparent symbol
│
└── preview.html                     # Visual Showcase & Verification Gallery
```

---

## 🚀 Platform Integration Guide

### 1. Apple macOS
- **Native ICNS**: Copy `assets/icons/macos/icon.icns` into your macOS app bundle or Xcode project resources.
- **Tauri Integration**: `clients/rust/tauri-shell/icons/icon.icns` is already synchronized and active.

### 2. Apple iOS / iPadOS
- **Xcode Asset Catalog**: Replace `Assets.xcassets/AppIcon.appiconset` with `assets/icons/ios/AppIcon.appiconset`.
- **App Store Connect**: Upload `assets/icons/ios/AppIcon-1024.png` (strictly 1024×1024 24-bit RGB without alpha, per Apple App Store requirements).

### 3. Microsoft Windows
- **Win32 / Tauri Executable**: Embedded `icon.ico` contains 7 resolution layers (16, 24, 32, 48, 64, 128, 256) with 32-bit RGBA alpha.
- **MSIX / Windows 11 AppX Manifest**: Point square logo definitions to `assets/icons/windows/Square*Logo.png`.

### 4. Linux (GNOME / KDE / Wayland)
- **System Installation**:
  ```bash
  sudo cp -r assets/icons/linux/hicolor/* /usr/share/icons/hicolor/
  sudo cp assets/icons/linux/eclipticrd.desktop /usr/share/applications/
  sudo gtk-update-icon-cache /usr/share/icons/hicolor/
  ```
- **Scalable Vector**: `/usr/share/icons/hicolor/scalable/apps/eclipticrd.svg` renders crisply on 4K/HiDPI displays.

### 5. Tauri v2 Desktop Client (`tauri-shell`)
- All required bundle assets have been synchronized directly into `clients/rust/tauri-shell/icons/`:
  - `icon.icns`
  - `icon.ico`
  - `icon.png` (512x512)
  - `icon-1024.png`
  - `icon-1024-8bit.png` (8-bit RGBA, compatible with Tauri)
  - `32x32.png`, `128x128.png`, `128x128@2x.png`
  - `src-tauri/icons/icon.png`
