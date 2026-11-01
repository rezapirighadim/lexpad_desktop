import Foundation

/*
 Subtitles beside a film, in the sandboxed Mac App Store build.

 A sandboxed app may read only what the learner chose, dropped or opened. When
 a film opens, macOS also lets the app read a "related item": a file with the
 same name and another extension (Film.mkv → Film.srt), when the app's
 Info.plist declares that type with NSIsRelatedItemType (Info.mas.plist,
 Subtitles) and asks through NSFileCoordinator with a file presenter whose
 primaryPresentedItemURL is the film. The presenter stays registered for the
 rest of the run, so the subtitle stays readable while it is played.

 lexpad_related_grant(film, subtitle) answers 1 when the subtitle exists and
 is readable now, 0 otherwise. Called by media/files.rs only in the Mac App
 Store build, and only for the same-name candidates it builds itself.
 */

final class RelatedPresenter: NSObject, NSFilePresenter {
    let primaryPresentedItemURL: URL?
    let presentedItemURL: URL?
    let presentedItemOperationQueue = OperationQueue()

    init(primary: URL, item: URL) {
        primaryPresentedItemURL = primary
        presentedItemURL = item
    }
}

/// The presenters kept for this run, at most `maxPresenters` (oldest go first).
private var presenters: [RelatedPresenter] = []
private let presentersLock = NSLock()
private let maxPresenters = 256

@_cdecl("lexpad_related_grant")
public func lexpad_related_grant(_ film: UnsafePointer<CChar>, _ subtitle: UnsafePointer<CChar>) -> Int32 {
    let filmURL = URL(fileURLWithPath: String(cString: film))
    let itemURL = URL(fileURLWithPath: String(cString: subtitle))
    // Only a true related item: the same folder and the same name.
    guard filmURL.deletingLastPathComponent() == itemURL.deletingLastPathComponent(),
          filmURL.deletingPathExtension().lastPathComponent == itemURL.deletingPathExtension().lastPathComponent
    else { return 0 }
    if FileManager.default.isReadableFile(atPath: itemURL.path) { return 1 }
    let presenter = RelatedPresenter(primary: filmURL, item: itemURL)
    NSFileCoordinator.addFilePresenter(presenter)
    var readable = false
    var error: NSError?
    NSFileCoordinator(filePresenter: presenter).coordinate(readingItemAt: itemURL, options: [], error: &error) { url in
        readable = FileManager.default.isReadableFile(atPath: url.path)
    }
    guard readable else {
        NSFileCoordinator.removeFilePresenter(presenter)
        return 0
    }
    presentersLock.lock()
    presenters.append(presenter)
    if presenters.count > maxPresenters {
        NSFileCoordinator.removeFilePresenter(presenters.removeFirst())
    }
    presentersLock.unlock()
    return 1
}
