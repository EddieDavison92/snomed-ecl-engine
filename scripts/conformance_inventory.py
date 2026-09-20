"""Inventory every production in the pinned brief and long ECL grammars."""
import argparse
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent.parent
IMPLEMENTED = set("conjunctionExpressionConstraint disjunctionExpressionConstraint exclusionExpressionConstraint eclConceptReference conceptId wildCard descendantOf descendantOrSelfOf childOf childOrSelfOf ancestorOf ancestorOrSelfOf parentOf parentOrSelfOf conjunction disjunction exclusion sctId".split())
PARTIAL = set("expressionConstraint compoundExpressionConstraint subExpressionConstraint eclFocusConcept constraintOperator term ws mws comment".split())
REFINEMENTS = set("refinedExpressionConstraint dottedExpressionConstraint dottedExpressionAttribute dot top bottom eclRefinement conjunctionRefinementSet disjunctionRefinementSet subRefinement eclAttributeSet conjunctionAttributeSet disjunctionAttributeSet subAttributeSet eclAttributeGroup eclAttribute cardinality minValue to maxValue many reverseFlag eclAttributeName expressionComparisonOperator numericComparisonOperator stringComparisonOperator booleanComparisonOperator concreteString concreteStringSet concreteStringCharacters numericValue integerValue decimalValue nonNegativeIntegerValue booleanValue".split())
# These rules have implementation evidence, but remaining alternatives and semantics need review.
PARTIAL |= REFINEMENTS
MEMBERSHIP = {"refsetOperator", "memberOf", "refsetContainingAny"}
PARTIAL |= MEMBERSHIP
CONCEPT_FILTERS = set("conceptFilterConstraint conceptFilter definitionStatusFilter definitionStatusIdFilter definitionStatusIdKeyword definitionStatusTokenFilter definitionStatusKeyword definitionStatusToken definitionStatusTokenSet primitiveToken definedToken moduleFilter moduleIdKeyword effectiveTimeFilter effectiveTimeKeyword timeValue timeValueSet year month day activeFilter activeKeyword activeValue activeTrueValue activeFalseValue".split())
PARTIAL |= CONCEPT_FILTERS
DESCRIPTION_FILTERS = set("descriptionFilterConstraint descriptionFilter descriptionIdFilter descriptionIdKeyword descriptionId descriptionIdSet languageFilter language languageCode languageCodeSet typeFilter typeIdFilter typeId typeTokenFilter type typeToken typeTokenSet synonym fullySpecifiedName definition dialectFilter dialectIdFilter dialectId dialectAliasFilter dialect dialectAlias dialectAliasSet dialectIdSet acceptabilitySet acceptabilityConceptReferenceSet acceptabilityTokenSet acceptabilityToken acceptable preferred".split())
PARTIAL |= DESCRIPTION_FILTERS
TERM_FILTERS = set("termFilter termKeyword typedSearchTerm typedSearchTermSet wild matchKeyword matchSearchTerm matchSearchTermSet wildSearchTerm wildSearchTermSet escapedWildChar".split())
PARTIAL |= TERM_FILTERS
MEMBER_FILTERS = set("refsetFieldNameSet refsetFieldName memberFilterConstraint memberFilter memberFieldFilter timeComparisonOperator".split())
PARTIAL |= MEMBER_FILTERS
BOUNDARIES = {
    "expressionConstraint": "expressions", "eclRefinement": "refinements",
    "descriptionFilterConstraint": "description_filters", "conceptFilterConstraint": "concept_filters",
    "memberFilterConstraint": "member_filters", "historySupplement": "history",
    "numericValue": "value_syntax", "sctId": "lexical",
}


def inventory():
    grammars = []
    for name in ("abnf-brief.txt", "abnf-long.txt"):
        path = ROOT / "references/snomed-expression-constraint-language/syntax" / name
        raw = path.read_text(encoding="utf-8-sig").encode("utf-8")
        group = "expressions"
        rules = []
        for rule in re.findall(r"^([A-Za-z][A-Za-z0-9-]*)\s*=", raw.decode("utf-8-sig"), re.M):
            group = BOUNDARIES.get(rule, group)
            status = "implemented" if rule in IMPLEMENTED else "partial" if rule in PARTIAL else "pending"
            rules.append({"production": rule, "area": group, "status": status,
                          "evidence": "tests/member_filters.rs" if rule in MEMBER_FILTERS else "tests/term_filters.rs" if rule in TERM_FILTERS else "tests/description_filters.rs" if rule in DESCRIPTION_FILTERS else "tests/concept_filters.rs" if rule in CONCEPT_FILTERS else "tests/membership.rs" if rule in MEMBERSHIP else "tests/refinements.rs" if rule in REFINEMENTS else "tests/basic_ecl.rs" if status != "pending" else None})
        assert len({r["production"] for r in rules}) == len(rules)
        grammars.append({"file": name, "sha256_utf8_lf": hashlib.sha256(raw).hexdigest(), "productions": rules})
    return {"version": "2.3", "reference_commit": "b0e07105ae395821bcc953f3d6084b57dc7bef2c",
            "complete": False,
            "status_meaning": "Development coverage only. Production coverage does not prove semantic conformance. Partial includes untested lexical edges and alternatives. Every pending rule remains required.",
            "grammars": grammars}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    path = ROOT / "validation/ecl-conformance.json"
    result = inventory()
    if args.check:
        if json.loads(path.read_text(encoding="utf-8-sig")) != result:
            raise SystemExit("Conformance inventory differs from pinned grammar or coverage declarations")
    else:
        path.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"grammars": len(result["grammars"]), "productions": [len(g["productions"]) for g in result["grammars"]], "complete": False}))
