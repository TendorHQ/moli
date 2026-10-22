from __future__ import annotations

from copy import deepcopy
import json
from typing import Any

from ..assertions import SmokeError, assert_equal
from ..config import REPO_ROOT, is_moli_browser
from ..state import SmokeState


_FIXTURES = REPO_ROOT / "moli-renderer-v8/tests/fixtures"
_CHROMIUM_REFLECTION = json.loads(
    (_FIXTURES / "constructor-reflection-chromium-145.json").read_text()
)
_REALM_CONSTRUCTORS = (
    "DOMParser",
    "RTCIceCandidate",
    "RTCSessionDescription",
    "RTCPeerConnectionIceEvent",
    "RTCPeerConnectionIceErrorEvent",
)
_REALM_TARGETS = ("foreign", "boundProxy", "proxyBound", "nestedBound")


def validate_constructor_reflection(
    observed: dict[str, Any], *, allow_proxy_source: bool
) -> list[dict[str, str]]:
    normalized = deepcopy(observed)
    rows = normalized["constructors"]
    expected_rows = _CHROMIUM_REFLECTION["constructors"]
    assert_equal(len(rows), len(expected_rows), "constructor reflection coverage")
    differences = []
    for row, expected in zip(rows, expected_rows):
        if row["source"] == expected["source"]:
            continue
        # V8's Function#toString does not unwrap callable Proxies. Only these
        # three source fields may differ; all other reflection is compared to
        # the executable Chromium probe. Named native source needs no waiver.
        if not allow_proxy_source or row["source"] != "function () { [native code] }":
            raise SmokeError(
                f"{row['name']} unexpected constructor source: {row['source']!r}"
            )
        differences.append(
            {
                "constructor": row["name"],
                "field": "source",
                "chromium": expected["source"],
                "observed": row["source"],
            }
        )
        row["source"] = expected["source"]
    assert_equal(normalized, _CHROMIUM_REFLECTION, "constructor reflection and inheritance")
    return differences


async def constructor_reflection_and_inheritance(state: SmokeState) -> dict[str, Any]:
    context = await state.browser.new_context()
    try:
        page = await context.new_page()
        await page.goto(f"{state.fixture}/plain", wait_until="load", timeout=10_000)
        observed = await page.evaluate(
            (_FIXTURES / "constructor-reflection.js").read_text()
        )
        differences = validate_constructor_reflection(
            observed, allow_proxy_source=is_moli_browser()
        )
        return {**observed, "knownDifferences": differences}
    finally:
        await context.close()


def validate_constructor_realms(
    observed: dict[str, Any], *, allow_chromium_object: bool
) -> list[dict[str, str]]:
    rows = observed["fallbacks"]
    assert_equal(
        [(row["target"], row["name"]) for row in rows],
        [(target, name) for target in _REALM_TARGETS for name in _REALM_CONSTRUCTORS],
        "constructor Realm coverage",
    )
    differences = []
    for row in rows:
        label = f"{row['name']} {row['target']}"
        assert_equal(row["prototypeReads"], 1, f"{label} prototype lookup count")
        assert_equal(row["nativeBrand"], True, f"{label} native brand")
        if row["targetInterface"] is True and row["targetObject"] is False:
            continue
        # WebIDL 3.8 requires the target Realm's interface prototype. The pinned
        # Chromium 145 executable instead selects that Realm's Object.prototype.
        # This exception must never accept a fallback from the caller's Realm.
        if (
            not allow_chromium_object
            or row["targetInterface"] is not False
            or row["targetObject"] is not True
        ):
            raise SmokeError(f"{label} incorrect target Realm fallback: {row!r}")
        differences.append(
            {
                "constructor": row["name"],
                "target": row["target"],
                "field": "prototype",
                "webidl": "target Realm interface prototype",
                "observed": "target Realm Object.prototype",
            }
        )
    revoked = observed["revoked"]
    assert_equal(
        [row["name"] for row in revoked],
        [*_REALM_CONSTRUCTORS, "BroadcastChannel"],
        "revoked explicit-prototype coverage",
    )
    for row in revoked:
        assert_equal(row["customPrototype"], True, f"{row['name']} explicit prototype")
        assert_equal(row["nativeBrand"], True, f"{row['name']} revoked-target native brand")
    return differences


async def constructor_newtarget_realms(state: SmokeState) -> dict[str, Any]:
    context = await state.browser.new_context()
    try:
        page = await context.new_page()
        await page.goto(f"{state.fixture}/plain", wait_until="load", timeout=10_000)
        observed = await page.evaluate((_FIXTURES / "constructor-realms.js").read_text())
        differences = validate_constructor_realms(
            observed, allow_chromium_object=not is_moli_browser()
        )
        return {**observed, "knownDifferences": differences}
    finally:
        await context.close()
