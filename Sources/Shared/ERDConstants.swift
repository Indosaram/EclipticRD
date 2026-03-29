import Foundation

public enum ERDConstants {
    public static let bonjourServiceType = "_eclipticrd._tcp"
    public static let bonjourDomain = "local."
    public static let tcpPort: UInt16 = 19730
    public static let udpPort: UInt16 = 19731

    public static let defaultBitrate = 8_000_000
    public static let minBitrate = 1_000_000
    public static let maxBitrate = 20_000_000
    public static let defaultFPS = 60
    public static let keyFrameInterval = 60

    public static let magic: UInt16 = 0xEC1D
    public static let packetHeaderSize = 12
    public static let maxPacketSize = 1400
    public static let maxPayloadSize = maxPacketSize - packetHeaderSize
    public static let maxChunksPerFrame: UInt16 = 1024

    public static let pixelFormat: UInt32 = 0x42475241 // kCVPixelFormatType_32BGRA

    public static let connectionTimeout: Double = 10.0
    public static let heartbeatInterval: Double = 2.0
    public static let frameAssemblyTimeout: Double = 1.0
    public static let inputBatchInterval: Double = 1.0 / 120.0

    public static let protocolVersion: UInt8 = 1
}
