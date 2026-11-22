import AppKit
import AuthenticationServices
import Foundation

/*
 Signing in without leaving the app, in the Mac App Store build only.

 App Review Guideline 4 (19 Nov 2026, on 0.3.33): a Mac App Store app may not
 send the learner to the default browser to sign in or register; it may use
 the web for that inside ASWebAuthenticationSession, and Sign in with Apple
 must finish without leaving the app. So this copy signs in two ways, both
 driven by src-tauri/src/signin_mas.rs:

 - lexpad_web_auth_start(url, scheme, window, ctx, callback): the same
   RFC 8252 connect page as the dmg (PKCE, state, lexpad://auth/callback), in
   ASWebAuthenticationSession's sheet over Lexpad's window. It shares the
   browser's cookies (prefersEphemeralWebBrowserSession false), so Google and
   Apple on that page see the learner as signed in. The session catches the
   callback itself; macOS does not hand it to the app's URL handler.
 - lexpad_apple_sign_in(nonceHash, window, ctx, callback): Sign in with Apple
   through AuthenticationServices' own sheet (ASAuthorizationAppleIDProvider),
   with the SHA-256 of a nonce the core keeps. The identity token goes to the
   API (POST /auth/apple) from the core; nothing is decided here.

 Each calls back exactly once with a status and a text: 0 and the callback
 URL or the Apple credential as JSON, 1 when the learner cancelled (or the
 core cancelled with lexpad_web_auth_cancel / lexpad_apple_cancel), 2 and a
 reason when it failed. The text is valid only during the call.
 */

public typealias LexpadSignInReply = @convention(c) (UInt64, Int32, UnsafePointer<CChar>) -> Void

private let statusDone: Int32 = 0
private let statusCancelled: Int32 = 1
private let statusFailed: Int32 = 2

/// The window a sheet is shown over: the one that asked, while it is on
/// screen, else whichever Lexpad window is in front.
final class SignInAnchor: NSObject, ASWebAuthenticationPresentationContextProviding,
    ASAuthorizationControllerPresentationContextProviding
{
    private let window: NSWindow?

    init(window: NSWindow?) {
        self.window = window
    }

    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        anchor()
    }

    func presentationAnchor(for controller: ASAuthorizationController) -> ASPresentationAnchor {
        anchor()
    }

    private func anchor() -> NSWindow {
        if let w = window, w.isVisible { return w }
        if let w = NSApp.keyWindow { return w }
        if let w = NSApp.mainWindow { return w }
        if let w = NSApp.windows.first(where: { $0.isVisible && $0.canBecomeKey }) { return w }
        return window ?? NSWindow()
    }
}

/// One sign-in's answer, given once whatever happens.
final class SignInReply {
    private let ctx: UInt64
    private let callback: LexpadSignInReply
    private var given = false
    private let lock = NSLock()

    init(ctx: UInt64, callback: LexpadSignInReply) {
        self.ctx = ctx
        self.callback = callback
    }

    func give(_ status: Int32, _ text: String) {
        lock.lock()
        let first = !given
        given = true
        lock.unlock()
        guard first else { return }
        text.withCString { callback(ctx, status, $0) }
    }
}

private func window(from pointer: UnsafeMutableRawPointer?) -> NSWindow? {
    guard let pointer else { return nil }
    return Unmanaged<NSWindow>.fromOpaque(pointer).takeUnretainedValue()
}

// ---------------------------------------------------------- the connect page

/// The sheet on screen now, its answer and the anchor it needs kept alive
/// (the session holds its presentation provider weakly). Main thread only.
private var webSession: (session: ASWebAuthenticationSession, reply: SignInReply, anchor: SignInAnchor)?

@_cdecl("lexpad_web_auth_start")
public func lexpad_web_auth_start(
    _ url: UnsafePointer<CChar>,
    _ scheme: UnsafePointer<CChar>,
    _ window: UnsafeMutableRawPointer?,
    _ ctx: UInt64,
    _ callback: LexpadSignInReply
) {
    let address = String(cString: url)
    let callbackScheme = String(cString: scheme)
    let reply = SignInReply(ctx: ctx, callback: callback)
    let asker = window
    DispatchQueue.main.async {
        guard let target = URL(string: address), !callbackScheme.isEmpty else {
            reply.give(statusFailed, "not a sign-in address")
            return
        }
        // A new sign-in replaces one still on screen; that one ends cancelled.
        if let old = webSession {
            webSession = nil
            old.session.cancel()
            old.reply.give(statusCancelled, "")
        }
        let done: ASWebAuthenticationSession.CompletionHandler = { answer, error in
            DispatchQueue.main.async {
                if webSession?.reply === reply { webSession = nil }
            }
            if let answer {
                reply.give(statusDone, answer.absoluteString)
            } else if let error = error as? ASWebAuthenticationSessionError, error.code == .canceledLogin {
                reply.give(statusCancelled, "")
            } else {
                reply.give(statusFailed, error.map { "\($0)" } ?? "the sheet ended without an answer")
            }
        }
        let session: ASWebAuthenticationSession
        if #available(macOS 14.4, *) {
            session = ASWebAuthenticationSession(url: target, callback: .customScheme(callbackScheme), completionHandler: done)
        } else {
            session = ASWebAuthenticationSession(url: target, callbackURLScheme: callbackScheme, completionHandler: done)
        }
        let anchor = SignInAnchor(window: lexpadWindow(asker))
        session.presentationContextProvider = anchor
        // The browser's own cookies: a learner signed in to Google, Apple or
        // lexpad.app there is signed in here too.
        session.prefersEphemeralWebBrowserSession = false
        webSession = (session, reply, anchor)
        if !session.start() {
            webSession = nil
            reply.give(statusFailed, "the sign-in sheet could not start")
        }
    }
}

@_cdecl("lexpad_web_auth_cancel")
public func lexpad_web_auth_cancel() {
    DispatchQueue.main.async {
        guard let current = webSession else { return }
        webSession = nil
        current.session.cancel()
        current.reply.give(statusCancelled, "")
    }
}

private func lexpadWindow(_ pointer: UnsafeMutableRawPointer?) -> NSWindow? {
    window(from: pointer)
}

// ------------------------------------------------------- Sign in with Apple

final class AppleSignIn: NSObject, ASAuthorizationControllerDelegate {
    let reply: SignInReply
    let anchor: SignInAnchor
    var controller: ASAuthorizationController?

    init(reply: SignInReply, anchor: SignInAnchor) {
        self.reply = reply
        self.anchor = anchor
    }

    func authorizationController(controller: ASAuthorizationController, didCompleteWithAuthorization authorization: ASAuthorization) {
        end()
        guard let credential = authorization.credential as? ASAuthorizationAppleIDCredential,
              let tokenData = credential.identityToken,
              let token = String(data: tokenData, encoding: .utf8), !token.isEmpty
        else {
            reply.give(statusFailed, "Apple gave no identity token")
            return
        }
        var out: [String: String] = ["idToken": token]
        if let codeData = credential.authorizationCode, let code = String(data: codeData, encoding: .utf8), !code.isEmpty {
            out["code"] = code
        }
        // Apple gives the name only on the first authorisation.
        let name = [credential.fullName?.givenName, credential.fullName?.familyName]
            .compactMap { $0?.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
            .joined(separator: " ")
        if !name.isEmpty { out["displayName"] = name }
        guard let data = try? JSONSerialization.data(withJSONObject: out),
              let text = String(data: data, encoding: .utf8)
        else {
            reply.give(statusFailed, "the credential could not be read")
            return
        }
        reply.give(statusDone, text)
    }

    func authorizationController(controller: ASAuthorizationController, didCompleteWithError error: Error) {
        end()
        if let error = error as? ASAuthorizationError, error.code == .canceled {
            reply.give(statusCancelled, "")
        } else {
            let ns = error as NSError
            reply.give(statusFailed, "\(ns.domain) \(ns.code): \(ns.localizedDescription)")
        }
    }

    private func end() {
        DispatchQueue.main.async { [self] in
            if appleSignIn === self { appleSignIn = nil }
        }
    }
}

/// The Apple sheet on screen now (its delegate is held weakly). Main thread only.
private var appleSignIn: AppleSignIn?

@_cdecl("lexpad_apple_sign_in")
public func lexpad_apple_sign_in(
    _ nonceHash: UnsafePointer<CChar>,
    _ window: UnsafeMutableRawPointer?,
    _ ctx: UInt64,
    _ callback: LexpadSignInReply
) {
    let nonce = String(cString: nonceHash)
    let reply = SignInReply(ctx: ctx, callback: callback)
    let asker = window
    DispatchQueue.main.async {
        if let old = appleSignIn {
            appleSignIn = nil
            old.controller?.cancel()
            old.reply.give(statusCancelled, "")
        }
        let request = ASAuthorizationAppleIDProvider().createRequest()
        request.requestedScopes = [.fullName, .email]
        request.nonce = nonce
        let flow = AppleSignIn(reply: reply, anchor: SignInAnchor(window: lexpadWindow(asker)))
        let controller = ASAuthorizationController(authorizationRequests: [request])
        controller.delegate = flow
        controller.presentationContextProvider = flow.anchor
        flow.controller = controller
        appleSignIn = flow
        controller.performRequests()
    }
}

@_cdecl("lexpad_apple_cancel")
public func lexpad_apple_cancel() {
    DispatchQueue.main.async {
        guard let current = appleSignIn else { return }
        appleSignIn = nil
        current.controller?.cancel()
        current.reply.give(statusCancelled, "")
    }
}
