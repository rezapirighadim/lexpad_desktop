import AppKit

// A host for the StoreKit tests: no Dock icon (LSUIElement), and one window
// that is never put on screen, for StoreKit to attach its (disabled) sheet to.
final class Delegate: NSObject, NSApplicationDelegate {
    var window: NSWindow?
    func applicationDidFinishLaunching(_ notification: Notification) {
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 10, height: 10), styleMask: [.titled], backing: .buffered, defer: true)
    }
}
let delegate = Delegate()
NSApplication.shared.delegate = delegate
NSApplication.shared.run()
