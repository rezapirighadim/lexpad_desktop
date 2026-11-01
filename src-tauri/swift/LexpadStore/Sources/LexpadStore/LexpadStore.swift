import AppKit
import Foundation
import StoreKit

/*
 Lexpad Pro through Apple's in-app purchase, for the Mac App Store build only
 (Cargo feature `mas`). App Store Review Guideline 3.1.1 allows only Apple's
 own purchase for digital goods sold inside a Mac App Store app, and 3.1.3(f)
 asks a multiplatform service to offer its subscription there too.

 This is the iPhone shell's StoreKitPlugin.swift (lexpad_ios) for the Mac:
 the same StoreKit 2 calls, the same answers, the same error codes, so the
 web app's features/billing/appstore.ts drives both. Products, prices and the
 server are shared through Universal Purchase: the Mac build carries the
 iPhone app's bundle id (app.lexpad.ios) and its subscriptions.

 Nothing here decides that anybody has paid. A purchase hands back the
 transaction id and Apple's signed copy; the web app sends both to
 POST /billing/apple/verify, and only after the server answers 200 does it ask
 for `finish`. An unfinished transaction comes back through
 Transaction.updates and `unfinished`, so a server out of reach is a wait,
 never lost money.

 The bridge to Rust is two C functions (src-tauri/src/store.rs):
 - lexpad_store_call(method, json, ctx, callback): runs one method and calls
   back once with {"ok": value} or {"error": {code, message, data: {detail}}}.
 - lexpad_store_listen(callback): starts Transaction.updates for the life of
   the app; each verified transaction is called back as JSON.
 Strings passed to a callback are valid only during the call.
 */

public typealias LexpadStoreReply = @convention(c) (UInt64, UnsafePointer<CChar>) -> Void
public typealias LexpadStoreEvent = @convention(c) (UnsafePointer<CChar>) -> Void

/// Where a subscription bought here is changed or cancelled: Apple's page,
/// which macOS opens in the App Store app. Apple is the only seller that can.
let subscriptionsURL = URL(string: "https://apps.apple.com/account/subscriptions")!

struct StoreFailure: Error {
    let code: String
    let message: String
    let detail: String?
}

private var updatesTask: Task<Void, Never>?

@_cdecl("lexpad_store_listen")
public func lexpad_store_listen(_ callback: LexpadStoreEvent) {
    guard updatesTask == nil else { return }
    updatesTask = Task.detached {
        for await result in Transaction.updates {
            // An unverified transaction is not passed on: the server would
            // refuse it, and it stays unfinished for StoreKit to offer again.
            guard case .verified(let transaction) = result else { continue }
            emit(describe(transaction, signed: result.jwsRepresentation), callback)
        }
    }
}

@_cdecl("lexpad_store_call")
public func lexpad_store_call(
    _ method: UnsafePointer<CChar>,
    _ json: UnsafePointer<CChar>,
    _ ctx: UInt64,
    _ callback: LexpadStoreReply
) {
    let name = String(cString: method)
    let options = parseOptions(String(cString: json))
    Task.detached {
        let reply: [String: Any]
        do {
            reply = ["ok": try await run(name, options)]
        } catch let failure as StoreFailure {
            reply = ["error": shape(failure)]
        } catch {
            reply = ["error": shape(StoreFailure(code: "failed", message: error.localizedDescription, detail: clip(explain(error))))]
        }
        let text = encode(reply)
        text.withCString { callback(ctx, $0) }
    }
}

func parseOptions(_ text: String) -> [String: Any] {
    guard let data = text.data(using: .utf8),
          let object = try? JSONSerialization.jsonObject(with: data),
          let dict = object as? [String: Any] else { return [:] }
    return dict
}

func encode(_ value: Any) -> String {
    guard JSONSerialization.isValidJSONObject(value),
          let data = try? JSONSerialization.data(withJSONObject: value),
          let text = String(data: data, encoding: .utf8) else {
        return #"{"error":{"code":"failed","message":"The App Store's answer could not be read.","data":{}}}"#
    }
    return text
}

func emit(_ value: [String: Any], _ callback: LexpadStoreEvent) {
    encode(value).withCString { callback($0) }
}

func shape(_ failure: StoreFailure) -> [String: Any] {
    var data: [String: Any] = [:]
    if let detail = failure.detail { data["detail"] = detail }
    return ["code": failure.code, "message": failure.message, "data": data]
}

/// One method, by the name the web app's plugin interface uses.
func run(_ name: String, _ options: [String: Any]) async throws -> Any {
    switch name {
    case "canMakePayments":
        return ["allowed": AppStore.canMakePayments]
    case "products":
        return ["products": try await products(options["productIds"] as? [String] ?? [])]
    case "purchase":
        return try await purchase(options)
    case "finish":
        return ["finished": try await finish(options["transactionId"] as? String)]
    case "unfinished":
        return ["transactions": await collect(Transaction.unfinished, autoRenewableOnly: false)]
    case "currentEntitlements":
        return ["transactions": await collect(Transaction.currentEntitlements, autoRenewableOnly: true)]
    case "sync":
        try await sync()
        return NSNull()
    case "manageSubscriptions":
        await MainActor.run { _ = NSWorkspace.shared.open(subscriptionsURL) }
        return NSNull()
    case "appTransactionId":
        return try await appTransactionId()
    default:
        throw StoreFailure(code: "failed", message: "Unknown store method.", detail: name)
    }
}

// MARK: - What can be bought

func products(_ ids: [String]) async throws -> [[String: Any]] {
    guard !ids.isEmpty else { return [] }
    let found: [Product]
    do {
        found = try await Product.products(for: ids)
    } catch {
        throw StoreFailure(code: "unavailable", message: error.localizedDescription, detail: clip(explain(error)))
    }
    return found.compactMap { product in
        guard product.type == .autoRenewable,
              let period = product.subscription?.subscriptionPeriod else { return nil }
        var item: [String: Any] = [
            "id": product.id,
            "displayName": product.displayName,
            "description": product.description,
            "displayPrice": product.displayPrice,
            "price": NSDecimalNumber(decimal: product.price).stringValue,
            "currencyCode": product.priceFormatStyle.currencyCode,
            "priceLocale": product.priceFormatStyle.locale.identifier,
            "period": ["unit": name(of: period.unit), "value": period.value],
        ]
        if let intro = product.subscription?.introductoryOffer {
            item["introductoryOffer"] = describe(intro)
        }
        let promos: [[String: Any]] = (product.subscription?.promotionalOffers ?? []).compactMap { offer in
            guard let id = offer.id else { return nil }
            var described = describe(offer)
            described["id"] = id
            return described
        }
        if !promos.isEmpty { item["promotionalOffers"] = promos }
        return item
    }
}

// MARK: - Buying

/// The purchase options a request asks for, checked before any sheet opens.
/// Exposed for the tests: the same refusals as the iPhone shell.
func purchaseOptions(_ options: [String: Any]) throws -> (String, Set<Product.PurchaseOption>) {
    guard let productId = options["productId"] as? String, !productId.isEmpty else {
        throw StoreFailure(code: "bad_product", message: "A product id is required.", detail: nil)
    }
    // Without the account the server could not tell whose purchase it is and
    // would refuse it after Apple took the money: no sheet at all.
    guard let raw = options["appAccountToken"] as? String, let account = UUID(uuidString: raw) else {
        throw StoreFailure(code: "bad_account", message: "The account could not be attached to the purchase.", detail: nil)
    }
    var set: Set<Product.PurchaseOption> = [.appAccountToken(account)]
    let eligibility = options["introductoryOfferEligibility"] as? String
    let promoId = options["promotionalOfferId"] as? String
    let promoJWS = options["promotionalOfferJws"] as? String
    if promoId != nil || promoJWS != nil {
        guard eligibility == nil else {
            throw StoreFailure(code: "offer_conflict", message: "An introductory and a promotional offer cannot be used together.", detail: nil)
        }
        guard let offerId = promoId, !offerId.isEmpty, let jws = promoJWS, !jws.isEmpty else {
            throw StoreFailure(code: "bad_offer", message: "A promotional offer needs both its id and its signature.", detail: nil)
        }
        guard let promo = promotionalOffer(offerId, compactJWS: jws) else {
            throw StoreFailure(code: "offer_unsupported", message: "This version of the App Store cannot apply the offer.", detail: nil)
        }
        set.formUnion(promo)
    }
    if let eligibility {
        // Fail closed: without the server's answer the introductory price
        // would go to somebody it was not meant for.
        guard !eligibility.isEmpty, let option = introductoryOfferEligibility(eligibility) else {
            throw StoreFailure(code: "offer_unsupported", message: "This version of the App Store cannot apply the offer.", detail: nil)
        }
        set.insert(option)
    }
    return (productId, set)
}

func purchase(_ options: [String: Any]) async throws -> [String: Any] {
    let (productId, set) = try purchaseOptions(options)
    do {
        guard let product = try await Product.products(for: [productId]).first else {
            throw StoreFailure(code: "unknown_product", message: "This plan is not on sale in the App Store.", detail: nil)
        }
        let result = try await buy(product, set)
        switch result {
        case .success(let verification):
            switch verification {
            case .verified(let transaction):
                var answer = describe(transaction, signed: verification.jwsRepresentation)
                answer["outcome"] = "purchased"
                return answer
            case .unverified(_, let error):
                throw StoreFailure(code: "unverified", message: error.localizedDescription, detail: clip(explain(error)))
            }
        case .userCancelled:
            return ["outcome": "cancelled"]
        case .pending:
            return ["outcome": "pending"]
        @unknown default:
            return ["outcome": "cancelled"]
        }
    } catch let failure as StoreFailure {
        throw failure
    } catch StoreKitError.userCancelled {
        return ["outcome": "cancelled"]
    } catch StoreKitError.networkError(let error) {
        throw StoreFailure(code: "network", message: error.localizedDescription, detail: clip(explain(error)))
    } catch {
        throw StoreFailure(code: "failed", message: error.localizedDescription, detail: clip(explain(error)))
    }
}

/// Buys with Apple's sheet attached to Lexpad's window. From macOS 15.2
/// StoreKit wants the window to confirm in, and without one it may refuse
/// ("Unable to Complete Request"); before that it finds the key window.
func buy(_ product: Product, _ set: Set<Product.PurchaseOption>) async throws -> Product.PurchaseResult {
    if #available(macOS 15.2, *) {
        let window: NSWindow? = await MainActor.run {
            NSApp.keyWindow ?? NSApp.mainWindow ?? NSApp.windows.first { $0.isVisible } ?? NSApp.windows.first
        }
        if let window {
            return try await product.purchase(confirmIn: window, options: set)
        }
    }
    return try await product.purchase(options: set)
}

/// Finishes one unfinished transaction, verified or not; false when it is
/// not among them (the normal answer for one already finished).
func finish(_ raw: String?) async throws -> Bool {
    guard let raw, let wanted = UInt64(raw) else {
        throw StoreFailure(code: "bad_transaction", message: "A transaction id is required.", detail: nil)
    }
    for await result in Transaction.unfinished {
        let transaction: Transaction
        switch result {
        case .verified(let one): transaction = one
        case .unverified(let one, _): transaction = one
        }
        if transaction.id == wanted {
            await transaction.finish()
            return true
        }
    }
    return false
}

func collect(_ sequence: Transaction.Transactions, autoRenewableOnly: Bool) async -> [[String: Any]] {
    var found: [[String: Any]] = []
    for await result in sequence {
        guard case .verified(let transaction) = result else { continue }
        if autoRenewableOnly && transaction.productType != .autoRenewable { continue }
        found.append(describe(transaction, signed: result.jwsRepresentation))
    }
    return found
}

/// Asks the App Store again; it can show the Apple Account sign-in, so the
/// web app calls it only from a Restore purchases click.
func sync() async throws {
    do {
        try await AppStore.sync()
    } catch StoreKitError.userCancelled {
        throw StoreFailure(code: "cancelled", message: "The App Store sign-in was cancelled.", detail: nil)
    } catch StoreKitError.networkError(let error) {
        throw StoreFailure(code: "network", message: error.localizedDescription, detail: clip(explain(error)))
    } catch {
        let ns = error as NSError
        let code: String
        if ns.domain == SKErrorDomain && ns.code == SKError.Code.paymentCancelled.rawValue {
            code = "cancelled"
        } else if ns.domain == NSURLErrorDomain {
            code = "network"
        } else {
            code = "failed"
        }
        throw StoreFailure(code: code, message: error.localizedDescription, detail: clip(explain(error)))
    }
}

// MARK: - Who is buying

/// An App Store id for this Apple Account, for the server to sign the
/// introductory-offer eligibility with: the cached app transaction, then a
/// refresh (may show the Apple Account sheet; only from a buy click), then
/// any verified transaction. Needs macOS 13 (AppTransaction).
func appTransactionId() async throws -> [String: Any] {
    guard #available(macOS 13.0, *) else {
        throw StoreFailure(code: "unavailable", message: "The App Store cannot identify this app on this macOS version.", detail: nil)
    }
    var trouble: [String] = []
    do {
        if let id = verifiedID(try await AppTransaction.shared, step: "shared", trouble: &trouble) {
            return ["appTransactionId": id, "source": "app"]
        }
    } catch {
        trouble.append("shared: \(explain(error))")
    }
    do {
        if let id = verifiedID(try await AppTransaction.refresh(), step: "refresh", trouble: &trouble) {
            return ["appTransactionId": id, "source": "refresh"]
        }
    } catch StoreKitError.userCancelled {
        throw StoreFailure(code: "cancelled", message: "The App Store sign-in was cancelled.", detail: nil)
    } catch {
        trouble.append("refresh: \(explain(error))")
    }
    for await result in Transaction.all {
        guard case .verified(let transaction) = result else { continue }
        let id = transaction.appTransactionID
        return ["appTransactionId": isDigits(id) ? id : String(transaction.id), "source": "transaction"]
    }
    trouble.append("transactions: none")
    throw StoreFailure(code: "failed", message: "The App Store could not identify this Apple Account.", detail: clip(trouble.joined(separator: "; ")))
}

@available(macOS 13.0, *)
func verifiedID(_ result: VerificationResult<AppTransaction>, step: String, trouble: inout [String]) -> String? {
    switch result {
    case .verified(let app):
        if isDigits(app.appTransactionID) { return app.appTransactionID }
        trouble.append("\(step): id not digits (length \(app.appTransactionID.count), \(app.environment.rawValue))")
        return nil
    case .unverified(let app, let error):
        trouble.append("\(step): unverified (\(app.environment.rawValue)): \(explain(error))")
        return nil
    }
}

func isDigits(_ value: String) -> Bool {
    return !value.isEmpty && value.count <= 32 && value.allSatisfy { $0.isASCII && $0.isNumber }
}

func explain(_ error: Error) -> String {
    let ns = error as NSError
    return "\(ns.domain) \(ns.code): \(String(describing: error))"
}

func clip(_ text: String) -> String {
    return text.count <= 300 ? text : String(text.prefix(300))
}

/// The purchase option carrying the server's eligibility JWS, or nil when this
/// build's SDK has no such option. StoreKit back-deploys the option to macOS 13
/// (the build's minimum), and only SDKs from Xcode 26 on declare it, hence the
/// compiler check and no OS check: an OS check here made every purchase of a
/// product with an introductory offer (the yearly plan) fail below macOS 15.4.
func introductoryOfferEligibility(_ compactJWS: String) -> Product.PurchaseOption? {
    #if compiler(>=6.2)
    return .introductoryOfferEligibility(compactJWS: compactJWS)
    #else
    return nil
    #endif
}

/// The purchase options for a server-signed promotional offer (back-deployed
/// like the eligibility option), or nil when this build's SDK has none.
func promotionalOffer(_ offerID: String, compactJWS: String) -> [Product.PurchaseOption]? {
    #if compiler(>=6.2)
    return Product.PurchaseOption.promotionalOffer(offerID, compactJWS: compactJWS)
    #else
    return nil
    #endif
}

// MARK: - Shapes

func describe(_ offer: Product.SubscriptionOffer) -> [String: Any] {
    return [
        "displayPrice": offer.displayPrice,
        "price": NSDecimalNumber(decimal: offer.price).stringValue,
        "paymentMode": name(of: offer.paymentMode),
        "period": ["unit": name(of: offer.period.unit), "value": offer.period.value],
        "periodCount": offer.periodCount,
    ]
}

func describe(_ transaction: Transaction, signed: String) -> [String: Any] {
    return [
        "transactionId": String(transaction.id),
        "originalTransactionId": String(transaction.originalID),
        "productId": transaction.productID,
        "signedTransaction": signed,
    ]
}

func name(of mode: Product.SubscriptionOffer.PaymentMode) -> String {
    switch mode {
    case .payUpFront: return "payUpFront"
    case .payAsYouGo: return "payAsYouGo"
    case .freeTrial: return "freeTrial"
    default: return mode.rawValue
    }
}

func name(of unit: Product.SubscriptionPeriod.Unit) -> String {
    switch unit {
    case .day: return "day"
    case .week: return "week"
    case .month: return "month"
    case .year: return "year"
    @unknown default: return "month"
    }
}
