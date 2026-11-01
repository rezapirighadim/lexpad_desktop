#!/usr/bin/env python3
"""Prepares the Mac App Store version in App Store Connect through the API
(docs/mac-app-store-owner-steps.md, step 4), after the owner's yes to the record
decision (step 1). It never submits anything.

    scripts/mas-asc-listing.py            say what exists and what it would do
    scripts/mas-asc-listing.py --apply    add the macOS version 0.3.27 to the iPhone
                                          app's record (if there is none), set its
                                          listing text and App Review details from
                                          docs/mac-app-store-owner-steps.md

The review password is never set here: the owner types it in App Store Connect.
It uses the App Store Connect API client of lexpad_ios (scripts/asc-products.py,
in the sibling checkout ../ios) and the key in ~/.appstoreconnect/lexpad.env.
"""
import importlib.util
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
HELPER = HERE.parent.parent / "ios" / "scripts" / "asc-products.py"
if not HELPER.exists():
    sys.exit(f"mas-asc-listing: needs lexpad_ios's {HELPER}")
spec = importlib.util.spec_from_file_location("ascp", HELPER)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
m.load_env()
c = m.Client()
APP = "6822224295"  # "Lexpad: AI Vocabulary Notebook", the iPhone app's record
doc=open(HERE.parent / "docs" / "mac-app-store-owner-steps.md").read()
blocks=re.findall(r"```\n(.*?)\n```", doc, re.S)
desc=[b for b in blocks if b.startswith("Lexpad is a vocabulary notebook")][0]
kw=[b for b in blocks if b.startswith("vocabulary,")][0].strip()
notes=[b for b in blocks if b.startswith("Lexpad is a personal vocabulary")][0]
promo=re.search(r"\*\*Promotional text\*\*.*?\n\n> (.*?)\n\n", doc, re.S).group(1).replace("\n> "," ")
assert len(kw)<=100 and len(promo)<=170 and len(desc)<=4000 and len(notes)<=4000, (len(kw),len(promo),len(desc),len(notes))
vers=c.call("GET",f"/v1/apps/{APP}/appStoreVersions",query={"filter[platform]":"MAC_OS"})["data"]
if vers:
    v=vers[0]; print("existing mac version", v["attributes"]["versionString"], v["attributes"]["appStoreState"])
    if "--apply" not in sys.argv: print("would set the listing text and review details"); sys.exit()
else:
    if "--apply" not in sys.argv: print("would create MAC_OS 0.3.27"); sys.exit()
    v=c.call("POST","/v1/appStoreVersions",{"data":{"type":"appStoreVersions","attributes":{"platform":"MAC_OS","versionString":"0.3.27","releaseType":"MANUAL"},"relationships":{"app":m.rel("apps",APP)}}})["data"]
    print("created", v["id"], v["attributes"]["versionString"], v["attributes"]["appStoreState"])
locs=c.call("GET",f"/v1/appStoreVersions/{v['id']}/appStoreVersionLocalizations")["data"]
attrs={"description":desc,"keywords":kw,"promotionalText":promo,"supportUrl":"https://lexpad.app/support","marketingUrl":"https://lexpad.app"}
en=[l for l in locs if l["attributes"]["locale"]=="en-US"]
if en:
    c.call("PATCH",f"/v1/appStoreVersionLocalizations/{en[0]['id']}",{"data":{"type":"appStoreVersionLocalizations","id":en[0]["id"],"attributes":attrs}})
else:
    c.call("POST","/v1/appStoreVersionLocalizations",{"data":{"type":"appStoreVersionLocalizations","attributes":{"locale":"en-US",**attrs},"relationships":{"appStoreVersion":m.rel("appStoreVersions",v["id"])}}})
print("listing text set; other locales:", [l["attributes"]["locale"] for l in locs if l["attributes"]["locale"]!="en-US"])
ios=c.call("GET",f"/v1/apps/{APP}/appStoreVersions",query={"filter[platform]":"IOS"})["data"][0]
ir=c.call("GET",f"/v1/appStoreVersions/{ios['id']}/appStoreReviewDetail")["data"]["attributes"]
review={k:ir[k] for k in ["contactFirstName","contactLastName","contactPhone","contactEmail"]}
review.update({"demoAccountName":"apple-reviewer@lexpad.app","demoAccountRequired":True,"notes":notes})
try:
    rd=c.call("GET",f"/v1/appStoreVersions/{v['id']}/appStoreReviewDetail")["data"]
except m.ApiError:
    rd=None
if rd:
    c.call("PATCH",f"/v1/appStoreReviewDetails/{rd['id']}",{"data":{"type":"appStoreReviewDetails","id":rd["id"],"attributes":review}})
else:
    c.call("POST","/v1/appStoreReviewDetails",{"data":{"type":"appStoreReviewDetails","attributes":review,"relationships":{"appStoreVersion":m.rel("appStoreVersions",v["id"])}}})
print("review details set (password left empty for the owner)")
