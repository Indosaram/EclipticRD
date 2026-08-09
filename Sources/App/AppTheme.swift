import SwiftUI

struct ERDAppBackground: View {
    var body: some View {
        ERDTheme.background
            .ignoresSafeArea()
    }
}

enum ERDTheme {
    static let background = Color(red: 0.06, green: 0.07, blue: 0.09)
    static let surface = Color(red: 0.10, green: 0.11, blue: 0.14)
    static let elevatedSurface = Color(red: 0.12, green: 0.13, blue: 0.16)
    static let subduedSurface = Color(red: 0.09, green: 0.10, blue: 0.12)
    static let panelBorder = Color(red: 0.20, green: 0.22, blue: 0.27)
    static let softBorder = Color(red: 0.17, green: 0.18, blue: 0.22)
    static let strongBorder = Color(red: 0.25, green: 0.27, blue: 0.33)
    static let mutedText = Color.white.opacity(0.62)
    static let strongText = Color.white.opacity(0.95)

    static let blue = Color(red: 0.33, green: 0.57, blue: 0.94)
    static let teal = Color(red: 0.25, green: 0.69, blue: 0.64)
    static let amber = Color(red: 0.88, green: 0.62, blue: 0.28)
    static let red = Color(red: 0.84, green: 0.38, blue: 0.34)
    static let green = Color(red: 0.34, green: 0.72, blue: 0.49)

    enum Spacing {
        static let micro: CGFloat = 4
        static let fine: CGFloat = 6
        static let pillVertical: CGFloat = 7
        static let compact: CGFloat = 8
        static let small: CGFloat = 10
        static let row: CGFloat = 12
        static let field: CGFloat = 14
        static let section: CGFloat = 16
        static let card: CGFloat = 18
        static let panel: CGFloat = 22
        static let panelLarge: CGFloat = 24
        static let screen: CGFloat = 28
    }

    enum Radius {
        static let iconCompact: CGFloat = 10
        static let control: CGFloat = 12
        static let tile: CGFloat = 14
        static let card: CGFloat = 16
        static let rowCard: CGFloat = 18
        static let panel: CGFloat = 20
    }

    enum Border {
        static let hairline: CGFloat = 1
    }

    enum Typography {
        static let eyebrow = Font.system(size: 11, weight: .semibold, design: .rounded)
        static let pill = Font.system(size: 11, weight: .semibold, design: .rounded)
        static let caption = Font.system(size: 11)
        static let captionMedium = Font.system(size: 11, weight: .medium)
        static let detail = Font.system(size: 12)
        static let detailMedium = Font.system(size: 12, weight: .medium)
        static let body = Font.system(size: 13)
        static let bodyEmphasized = Font.system(size: 13, weight: .semibold)
        static let button = Font.system(size: 13, weight: .semibold)
        static let headline = Font.system(size: 15, weight: .semibold)
        static let title = Font.system(size: 24, weight: .bold)
        static let hero = Font.system(size: 32, weight: .bold, design: .rounded)
        static let metricValue = Font.system(size: 18, weight: .bold, design: .rounded)
        static let emphasizedData = Font.system(size: 24, weight: .bold, design: .monospaced)
        static let input = Font.system(size: 16, weight: .semibold, design: .monospaced)
        static let icon = Font.system(size: 14, weight: .semibold)
    }

    enum Layout {
        static let featureIconSize: CGFloat = 30
        static let rowIconSize: CGFloat = 38
        static let stateAccessorySize: CGFloat = 72
        static let dashboardMaxWidth: CGFloat = 980
        static let dashboardSidebarWidth: CGFloat = 320
        static let stateSurfaceWidth: CGFloat = 540
    }

    enum Motion {
        static let press = Animation.easeOut(duration: 0.14)
    }
}

struct ERDCardChrome {
    let fill: Color
    let stroke: Color
    let cornerRadius: CGFloat
    let lineWidth: CGFloat

    init(
        fill: Color,
        stroke: Color,
        cornerRadius: CGFloat,
        lineWidth: CGFloat = ERDTheme.Border.hairline
    ) {
        self.fill = fill
        self.stroke = stroke
        self.cornerRadius = cornerRadius
        self.lineWidth = lineWidth
    }

    static let panel = ERDCardChrome(
        fill: ERDTheme.surface,
        stroke: ERDTheme.panelBorder,
        cornerRadius: ERDTheme.Radius.panel
    )

    static let subdued = ERDCardChrome(
        fill: ERDTheme.subduedSurface,
        stroke: ERDTheme.softBorder,
        cornerRadius: ERDTheme.Radius.rowCard
    )

    static let elevated = ERDCardChrome(
        fill: ERDTheme.elevatedSurface,
        stroke: ERDTheme.softBorder,
        cornerRadius: ERDTheme.Radius.control
    )

    static func elevated(stroke: Color, cornerRadius: CGFloat = ERDTheme.Radius.control) -> ERDCardChrome {
        ERDCardChrome(
            fill: ERDTheme.elevatedSurface,
            stroke: stroke,
            cornerRadius: cornerRadius
        )
    }

    static func surface(fill: Color, stroke: Color, cornerRadius: CGFloat) -> ERDCardChrome {
        ERDCardChrome(fill: fill, stroke: stroke, cornerRadius: cornerRadius)
    }
}

private struct ERDCardBackgroundStyle: ViewModifier {
    let chrome: ERDCardChrome

    func body(content: Content) -> some View {
        content
            .background(
                RoundedRectangle(cornerRadius: chrome.cornerRadius, style: .continuous)
                    .fill(chrome.fill)
                    .overlay(
                        RoundedRectangle(cornerRadius: chrome.cornerRadius, style: .continuous)
                            .strokeBorder(chrome.stroke, lineWidth: chrome.lineWidth)
                    )
            )
    }
}

struct ERDIconBadge: View {
    let systemImage: String
    var tint: Color = ERDTheme.strongText
    var size: CGFloat = ERDTheme.Layout.featureIconSize
    var font: Font = ERDTheme.Typography.icon
    var chrome: ERDCardChrome = .elevated(stroke: ERDTheme.softBorder, cornerRadius: ERDTheme.Radius.iconCompact)

    var body: some View {
        Image(systemName: systemImage)
            .font(font)
            .foregroundStyle(tint)
            .frame(width: size, height: size)
            .erdCardBackground(chrome)
    }
}

struct ERDRowCard<Leading: View, RowContent: View, Trailing: View>: View {
    var alignment: VerticalAlignment = .top
    var spacing: CGFloat = ERDTheme.Spacing.row
    var padding: CGFloat = ERDTheme.Spacing.field
    var chrome: ERDCardChrome = .subdued

    let leading: Leading
    let content: RowContent
    let trailing: Trailing

    init(
        alignment: VerticalAlignment = .top,
        spacing: CGFloat = ERDTheme.Spacing.row,
        padding: CGFloat = ERDTheme.Spacing.field,
        chrome: ERDCardChrome = .subdued,
        @ViewBuilder leading: () -> Leading,
        @ViewBuilder content: () -> RowContent,
        @ViewBuilder trailing: () -> Trailing
    ) {
        self.alignment = alignment
        self.spacing = spacing
        self.padding = padding
        self.chrome = chrome
        self.leading = leading()
        self.content = content()
        self.trailing = trailing()
    }

    var body: some View {
        HStack(alignment: alignment, spacing: spacing) {
            leading

            content
                .frame(maxWidth: .infinity, alignment: .leading)

            trailing
        }
        .padding(padding)
        .erdCardBackground(chrome)
    }
}

extension ERDRowCard where Trailing == EmptyView {
    init(
        alignment: VerticalAlignment = .top,
        spacing: CGFloat = ERDTheme.Spacing.row,
        padding: CGFloat = ERDTheme.Spacing.field,
        chrome: ERDCardChrome = .subdued,
        @ViewBuilder leading: () -> Leading,
        @ViewBuilder content: () -> RowContent
    ) {
        self.init(
            alignment: alignment,
            spacing: spacing,
            padding: padding,
            chrome: chrome,
            leading: leading,
            content: content,
            trailing: { EmptyView() }
        )
    }
}

struct ERDFeatureRowCard: View {
    let title: String
    let detail: String
    let systemImage: String
    var tint: Color = ERDTheme.strongText
    var chrome: ERDCardChrome = .subdued
    var iconChrome: ERDCardChrome = .elevated(stroke: ERDTheme.softBorder, cornerRadius: ERDTheme.Radius.iconCompact)
    var padding: CGFloat = ERDTheme.Spacing.field

    var body: some View {
        ERDRowCard(padding: padding, chrome: chrome) {
            ERDIconBadge(
                systemImage: systemImage,
                tint: tint,
                chrome: iconChrome
            )
        } content: {
            VStack(alignment: .leading, spacing: ERDTheme.Spacing.micro) {
                Text(title)
                    .font(ERDTheme.Typography.bodyEmphasized)
                    .foregroundStyle(ERDTheme.strongText)

                Text(detail)
                    .font(ERDTheme.Typography.detail)
                    .foregroundStyle(ERDTheme.mutedText)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

struct ERDPanel<Content: View>: View {
    let padding: CGFloat
    let content: Content

    init(padding: CGFloat = ERDTheme.Spacing.panel, @ViewBuilder content: () -> Content) {
        self.padding = padding
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: ERDTheme.Spacing.card) {
            content
        }
        .padding(padding)
        .erdCardBackground(.panel)
    }
}

struct ERDSectionHeader: View {
    let eyebrow: String
    let title: String
    let detail: String

    var body: some View {
        VStack(alignment: .leading, spacing: ERDTheme.Spacing.compact) {
            Text(eyebrow.uppercased())
                .font(ERDTheme.Typography.eyebrow)
                .tracking(1.2)
                .foregroundStyle(ERDTheme.blue)

            Text(title)
                .font(ERDTheme.Typography.title)
                .foregroundStyle(ERDTheme.strongText)

            Text(detail)
                .font(ERDTheme.Typography.body)
                .foregroundStyle(ERDTheme.mutedText)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

struct ERDStatusPill: View {
    let title: String
    let systemImage: String
    let tint: Color

    var body: some View {
        HStack(spacing: ERDTheme.Spacing.compact) {
            Image(systemName: systemImage)
                .font(ERDTheme.Typography.captionMedium)

            Text(title)
                .font(ERDTheme.Typography.pill)
        }
        .foregroundStyle(Color.white.opacity(0.92))
        .padding(.horizontal, ERDTheme.Spacing.small)
        .padding(.vertical, ERDTheme.Spacing.pillVertical)
        .background(
            Capsule(style: .continuous)
                .fill(ERDTheme.elevatedSurface)
                .overlay(
                    Capsule(style: .continuous)
                        .strokeBorder(tint.opacity(0.26), lineWidth: ERDTheme.Border.hairline)
                )
        )
    }
}

struct ERDInfoRow: View {
    let label: String
    let value: String
    var emphasizeValue = false
    var tint: Color = ERDTheme.strongText

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(label)
                .font(ERDTheme.Typography.detailMedium)
                .foregroundStyle(ERDTheme.mutedText)

            Spacer(minLength: ERDTheme.Spacing.section)

            Text(value)
                .font(emphasizeValue ? ERDTheme.Typography.emphasizedData : ERDTheme.Typography.bodyEmphasized)
                .foregroundStyle(tint)
                .multilineTextAlignment(.trailing)
        }
    }
}

struct ERDActionButtonStyle: ButtonStyle {
    let tint: Color
    var isProminent = true

    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(ERDTheme.Typography.button)
            .foregroundStyle(
                isEnabled
                    ? (isProminent ? Color.white.opacity(0.96) : Color.white.opacity(0.90))
                    : Color.white.opacity(0.38)
            )
            .padding(.horizontal, ERDTheme.Spacing.section)
            .padding(.vertical, ERDTheme.Spacing.small)
            .background(
                RoundedRectangle(cornerRadius: ERDTheme.Radius.control, style: .continuous)
                    .fill(
                        isEnabled
                            ? (isProminent ? tint : ERDTheme.elevatedSurface)
                            : ERDTheme.subduedSurface
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: ERDTheme.Radius.control, style: .continuous)
                            .strokeBorder(
                                isEnabled
                                    ? (isProminent ? tint.opacity(0.24) : ERDTheme.panelBorder)
                                    : ERDTheme.softBorder,
                                lineWidth: ERDTheme.Border.hairline
                            )
                    )
            )
            .scaleEffect(isEnabled && configuration.isPressed ? 0.985 : 1)
            .animation(ERDTheme.Motion.press, value: configuration.isPressed)
    }
}

struct ERDMetricTile: View {
    let title: String
    let value: String
    let systemImage: String
    let tint: Color

    var body: some View {
        VStack(alignment: .leading, spacing: ERDTheme.Spacing.small) {
            Label(title, systemImage: systemImage)
                .font(ERDTheme.Typography.captionMedium)
                .foregroundStyle(ERDTheme.mutedText)

            Text(value)
                .font(ERDTheme.Typography.metricValue)
                .foregroundStyle(Color.white.opacity(0.94))
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(ERDTheme.Spacing.field)
        .erdCardBackground(
            .elevated(stroke: tint.opacity(0.18), cornerRadius: ERDTheme.Radius.tile)
        )
    }
}

struct ERDWorkspaceHeroPanel<Content: View>: View {
    let eyebrow: String
    let title: String
    let detail: String
    let status: String
    let statusSystemImage: String
    let statusTint: Color
    let content: Content

    init(
        eyebrow: String,
        title: String,
        detail: String,
        status: String,
        statusSystemImage: String,
        statusTint: Color,
        @ViewBuilder content: () -> Content
    ) {
        self.eyebrow = eyebrow
        self.title = title
        self.detail = detail
        self.status = status
        self.statusSystemImage = statusSystemImage
        self.statusTint = statusTint
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: ERDTheme.Spacing.panel) {
            VStack(alignment: .leading, spacing: ERDTheme.Spacing.section) {
                HStack(alignment: .top, spacing: ERDTheme.Spacing.section) {
                    VStack(alignment: .leading, spacing: ERDTheme.Spacing.small) {
                        Text(eyebrow.uppercased())
                            .font(ERDTheme.Typography.eyebrow)
                            .tracking(1.3)
                            .foregroundStyle(ERDTheme.blue)

                        Text(title)
                            .font(ERDTheme.Typography.hero)
                            .foregroundStyle(ERDTheme.strongText)
                            .fixedSize(horizontal: false, vertical: true)

                        Text(detail)
                            .font(ERDTheme.Typography.body)
                            .foregroundStyle(ERDTheme.mutedText)
                            .fixedSize(horizontal: false, vertical: true)
                    }

                    Spacer(minLength: ERDTheme.Spacing.section)

                    ERDStatusPill(title: status, systemImage: statusSystemImage, tint: statusTint)
                }
            }

            content
        }
        .padding(ERDTheme.Spacing.panelLarge)
        .erdCardBackground(.panel)
    }
}

struct ERDConnectionStateSurface<Accessory: View, Actions: View, Supporting: View>: View {
    let accent: Color
    let icon: String
    let title: String
    let message: String
    let footer: String
    let actions: Actions
    let supporting: Supporting
    let accessory: Accessory

    init(
        accent: Color,
        icon: String,
        title: String,
        message: String,
        footer: String,
        @ViewBuilder actions: () -> Actions,
        @ViewBuilder supporting: () -> Supporting,
        @ViewBuilder accessory: () -> Accessory
    ) {
        self.accent = accent
        self.icon = icon
        self.title = title
        self.message = message
        self.footer = footer
        self.actions = actions()
        self.supporting = supporting()
        self.accessory = accessory()
    }

    var body: some View {
        ERDPanel(padding: ERDTheme.Spacing.panelLarge) {
            HStack(alignment: .top, spacing: ERDTheme.Spacing.card) {
                accessory

                VStack(alignment: .leading, spacing: ERDTheme.Spacing.section) {
                    ERDStatusPill(title: title, systemImage: icon, tint: accent)

                    VStack(alignment: .leading, spacing: ERDTheme.Spacing.compact) {
                        Text(message)
                            .font(ERDTheme.Typography.headline)
                            .foregroundStyle(ERDTheme.strongText)
                            .fixedSize(horizontal: false, vertical: true)

                        Text(footer)
                            .font(ERDTheme.Typography.detail)
                            .foregroundStyle(ERDTheme.mutedText)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }

            supporting

            actions
        }
        .frame(maxWidth: ERDTheme.Layout.stateSurfaceWidth, alignment: .leading)
    }
}

extension ERDConnectionStateSurface where Supporting == EmptyView {
    init(
        accent: Color,
        icon: String,
        title: String,
        message: String,
        footer: String,
        @ViewBuilder actions: () -> Actions,
        @ViewBuilder accessory: () -> Accessory
    ) {
        self.init(
            accent: accent,
            icon: icon,
            title: title,
            message: message,
            footer: footer,
            actions: actions,
            supporting: { EmptyView() },
            accessory: accessory
        )
    }
}

struct ERDStateAccessoryBadge<Inner: View>: View {
    let tint: Color
    let inner: Inner

    init(tint: Color, @ViewBuilder inner: () -> Inner) {
        self.tint = tint
        self.inner = inner()
    }

    var body: some View {
        inner
            .frame(
                width: ERDTheme.Layout.stateAccessorySize,
                height: ERDTheme.Layout.stateAccessorySize
            )
            .background(
                Circle()
                    .fill(ERDTheme.elevatedSurface)
                    .overlay(
                        Circle()
                            .strokeBorder(tint.opacity(0.24), lineWidth: ERDTheme.Border.hairline)
                    )
            )
    }
}

struct ERDInputFieldStyle: ViewModifier {
    func body(content: Content) -> some View {
        content
            .font(ERDTheme.Typography.input)
            .foregroundStyle(Color.white.opacity(0.95))
            .padding(.horizontal, ERDTheme.Spacing.field)
            .padding(.vertical, ERDTheme.Spacing.row)
            .erdCardBackground(
                .elevated(stroke: ERDTheme.strongBorder, cornerRadius: ERDTheme.Radius.control)
            )
    }
}

extension View {
    func erdCardBackground(_ chrome: ERDCardChrome) -> some View {
        modifier(ERDCardBackgroundStyle(chrome: chrome))
    }

    func erdInputField() -> some View {
        modifier(ERDInputFieldStyle())
    }
}
