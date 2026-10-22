from __future__ import annotations

from copy import deepcopy
import json
import unittest

from moli_cdp_smoke.assertions import SmokeError
from moli_cdp_smoke.config import REPO_ROOT
from moli_cdp_smoke.groups.constructor_semantics import (
    validate_constructor_realms,
    validate_constructor_reflection,
)


class ConstructorReflectionTests(unittest.TestCase):
    def setUp(self) -> None:
        path = (
            REPO_ROOT
            / "moli-renderer-v8/tests/fixtures/constructor-reflection-chromium-145.json"
        )
        self.chromium = json.loads(path.read_text())
        self.moli = deepcopy(self.chromium)
        for row in self.moli["constructors"]:
            row["source"] = "function () { [native code] }"

    def test_named_native_sources_match_both_engines(self) -> None:
        for allow in (False, True):
            self.assertEqual(
                validate_constructor_reflection(self.chromium, allow_proxy_source=allow),
                [],
            )

    def test_proxy_sources_are_explicit_differences_without_mutating_observation(self) -> None:
        original = deepcopy(self.moli)
        differences = validate_constructor_reflection(self.moli, allow_proxy_source=True)
        self.assertEqual(len(differences), 3)
        self.assertEqual({row["field"] for row in differences}, {"source"})
        self.assertEqual(self.moli, original)

    def test_chromium_cannot_use_the_proxy_source_exception(self) -> None:
        with self.assertRaises(SmokeError):
            validate_constructor_reflection(self.moli, allow_proxy_source=False)

    def test_identity_failures_are_not_hidden_by_the_proxy_exception(self) -> None:
        for field in ("instanceOf", "subclassPrototype", "prototypeConstructor"):
            with self.subTest(field=field):
                observed = deepcopy(self.moli)
                observed["constructors"][0][field] = False
                with self.assertRaises(SmokeError):
                    validate_constructor_reflection(observed, allow_proxy_source=True)

    def test_missing_fields_and_other_function_source_changes_are_rejected(self) -> None:
        missing = deepcopy(self.moli)
        del missing["constructors"][0]["subclassConstructor"]
        changed = deepcopy(self.moli)
        changed["builtinSource"] = "function () { [native code] }"
        for observed in (missing, changed):
            with self.assertRaises(SmokeError):
                validate_constructor_reflection(observed, allow_proxy_source=True)


class ConstructorRealmTests(unittest.TestCase):
    def setUp(self) -> None:
        names = [
            "DOMParser",
            "RTCIceCandidate",
            "RTCSessionDescription",
            "RTCPeerConnectionIceEvent",
            "RTCPeerConnectionIceErrorEvent",
        ]
        targets = ["foreign", "boundProxy", "proxyBound", "nestedBound"]
        self.standard = {
            "fallbacks": [
                {
                    "name": name,
                    "target": target,
                    "prototypeReads": 1,
                    "targetInterface": True,
                    "targetObject": False,
                    "nativeBrand": True,
                }
                for target in targets
                for name in names
            ],
            "revoked": [
                {"name": name, "customPrototype": True, "nativeBrand": True}
                for name in [*names, "BroadcastChannel"]
            ],
        }
        self.chromium = deepcopy(self.standard)
        for row in self.chromium["fallbacks"]:
            row["targetInterface"] = False
            row["targetObject"] = True

    def test_standard_interface_fallback_needs_no_exception(self) -> None:
        for allow in (False, True):
            self.assertEqual(
                validate_constructor_realms(self.standard, allow_chromium_object=allow),
                [],
            )

    def test_pinned_chromium_fallback_is_explicit_and_engine_specific(self) -> None:
        differences = validate_constructor_realms(
            self.chromium, allow_chromium_object=True
        )
        self.assertEqual(len(differences), 20)
        with self.assertRaises(SmokeError):
            validate_constructor_realms(self.chromium, allow_chromium_object=False)

    def test_caller_realm_fallback_is_never_accepted(self) -> None:
        observed = deepcopy(self.chromium)
        observed["fallbacks"][0]["targetObject"] = False
        with self.assertRaises(SmokeError):
            validate_constructor_realms(observed, allow_chromium_object=True)

    def test_extra_prototype_lookups_are_never_accepted(self) -> None:
        observed = deepcopy(self.chromium)
        observed["fallbacks"][0]["prototypeReads"] = 2
        with self.assertRaises(SmokeError):
            validate_constructor_realms(observed, allow_chromium_object=True)

    def test_revoked_explicit_prototype_is_not_a_known_difference(self) -> None:
        observed = deepcopy(self.chromium)
        observed["revoked"][0]["customPrototype"] = False
        with self.assertRaises(SmokeError):
            validate_constructor_realms(observed, allow_chromium_object=True)


if __name__ == "__main__":
    unittest.main()
