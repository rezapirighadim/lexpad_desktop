import StoreKit
import StoreKitTest
import XCTest
@testable import StoreKitHost

/// The Mac App Store bridge against StoreKit's local test environment
/// (SKTestSession with Lexpad.storekit, run hosted by Harness/ through scripts/mas-storekit-test.sh, a test fixture: its prices are the
/// fixture's, never shown anywhere). No Apple Account, no network, no sheet.
final class LexpadStoreTests: XCTestCase {
    var session: SKTestSession!

    override func setUpWithError() throws {
        let url = try XCTUnwrap(Bundle.main.url(forResource: "Lexpad", withExtension: "storekit"))
        session = try SKTestSession(contentsOf: url)
        session.disableDialogs = true
        session.clearTransactions()
    }

    func testRefusesAPurchaseWithoutAnAccount() {
        XCTAssertThrowsError(try purchaseOptions(["productId": "pro_monthly"])) { error in
            XCTAssertEqual((error as? StoreFailure)?.code, "bad_account")
        }
        XCTAssertThrowsError(try purchaseOptions([
            "productId": "pro_monthly",
            "appAccountToken": "00000000-0000-0000-0000-000000000001",
            "promotionalOfferId": "x",
        ])) { error in
            XCTAssertEqual((error as? StoreFailure)?.code, "bad_offer")
        }
        XCTAssertThrowsError(try purchaseOptions([
            "productId": "pro_monthly",
            "appAccountToken": "00000000-0000-0000-0000-000000000001",
            "promotionalOfferId": "x", "promotionalOfferJws": "y", "introductoryOfferEligibility": "z",
        ])) { error in
            XCTAssertEqual((error as? StoreFailure)?.code, "offer_conflict")
        }
    }

    /// The offer options are built on every macOS the build runs on (13 and
    /// later): StoreKit back-deploys them, and a refusal here would make every
    /// yearly purchase fail (`offer_unsupported`). The harness and build.rs
    /// both compile for macOS 13, so an OS check slipping back in fails to
    /// build or fails here.
    func testBuildsTheOfferOptionsOnEveryMacOS() throws {
        let account = "00000000-0000-0000-0000-000000000001"
        let (_, intro) = try purchaseOptions([
            "productId": "pro_yearly", "appAccountToken": account,
            "introductoryOfferEligibility": "header.payload.signature",
        ])
        XCTAssertEqual(intro.count, 2, "the account and the eligibility")
        let (_, promo) = try purchaseOptions([
            "productId": "pro_yearly", "appAccountToken": account,
            "promotionalOfferId": "winback", "promotionalOfferJws": "header.payload.signature",
        ])
        XCTAssertGreaterThan(promo.count, 1, "the account and the promotional offer")
        XCTAssertNotNil(introductoryOfferEligibility("a.b.c"))
        XCTAssertNotNil(promotionalOffer("winback", compactJWS: "a.b.c"))
    }

    func testListsBuysFinishesAndRestores() async throws {
        let listed = try await StoreKitHost.run("products", ["productIds": ["pro_monthly", "pro_yearly", "nope"]]) as! [String: Any]
        let products = try XCTUnwrap(listed["products"] as? [[String: Any]])
        XCTAssertEqual(Set(products.map { $0["id"] as! String }), ["pro_monthly", "pro_yearly"])
        let monthly = products.first { $0["id"] as? String == "pro_monthly" }!
        XCTAssertEqual(monthly["price"] as? String, "4.99")
        XCTAssertEqual((monthly["period"] as? [String: Any])?["unit"] as? String, "month")

        let token = "01890a5d-ac96-774b-bcce-b302099a8057"
        let bought = try await StoreKitHost.run("purchase", ["productId": "pro_monthly", "appAccountToken": token]) as! [String: Any]
        XCTAssertEqual(bought["outcome"] as? String, "purchased")
        let id = try XCTUnwrap(bought["transactionId"] as? String)
        XCTAssertEqual(bought["productId"] as? String, "pro_monthly")
        XCTAssertFalse((bought["signedTransaction"] as? String ?? "").isEmpty)
        // The account went with the purchase, as the server checks.
        let tx = try XCTUnwrap(session.allTransactions().first { String($0.identifier) == id })
        XCTAssertEqual(tx.productIdentifier, "pro_monthly")

        // Not finished until the server agrees: it is still unfinished.
        // StoreKit's test store records a purchase a moment after it answers.
        let open = await eventually { try await StoreKitHost.run("unfinished", [:]) as! [String: Any] } until: {
            ($0["transactions"] as! [[String: Any]]).contains { $0["transactionId"] as? String == id }
        }
        XCTAssertNotNil(open)

        let done = try await StoreKitHost.run("finish", ["transactionId": id]) as! [String: Any]
        XCTAssertEqual(done["finished"] as? Bool, true)
        let again = try await StoreKitHost.run("finish", ["transactionId": id]) as! [String: Any]
        XCTAssertEqual(again["finished"] as? Bool, false)

        let held = await eventually { try await StoreKitHost.run("currentEntitlements", [:]) as! [String: Any] } until: {
            ($0["transactions"] as! [[String: Any]]).contains { $0["productId"] as? String == "pro_monthly" }
        }
        XCTAssertNotNil(held)
    }

    func testTheCBridgeAnswersAsJSON() {
        let done = expectation(description: "reply")
        Reply.expect = done
        "canMakePayments".withCString { m in "{}".withCString { j in lexpad_store_call(m, j, 7, Reply.callback) } }
        wait(for: [done], timeout: 10)
        XCTAssertEqual(Reply.ctx, 7)
        XCTAssertTrue(Reply.text.hasPrefix(#"{"ok":{"allowed":"#), Reply.text)
    }
}

/// Asks again for up to five seconds until the answer satisfies `until`.
func eventually(_ ask: () async throws -> [String: Any], until: ([String: Any]) -> Bool) async -> [String: Any]? {
    for _ in 0..<50 {
        if let answer = try? await ask(), until(answer) { return answer }
        try? await Task.sleep(nanoseconds: 100_000_000)
    }
    return nil
}

enum Reply {
    nonisolated(unsafe) static var expect: XCTestExpectation?
    nonisolated(unsafe) static var ctx: UInt64 = 0
    nonisolated(unsafe) static var text = ""
    static let callback: LexpadStoreReply = { ctx, text in
        Reply.ctx = ctx
        Reply.text = String(cString: text)
        Reply.expect?.fulfill()
    }
}
