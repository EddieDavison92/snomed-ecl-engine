//! Lexical and syntactic edges of the pinned ECL 2.3 grammars. Parsing never proves evaluation.
use snomed_ecl_engine::ecl::{parse, Expr, ParseErrorKind};

fn same(a: &str, b: &str) {
    assert_eq!(
        parse(a).unwrap_or_else(|e| panic!("{a}: {e}")),
        parse(b).unwrap_or_else(|e| panic!("{b}: {e}")),
        "{a} versus {b}"
    );
}
fn syntax_error(query: &str) {
    match parse(query) {
        Err(e) if e.kind == ParseErrorKind::Syntax => {}
        other => panic!("{query}: expected a syntax error, got {other:?}"),
    }
}
/// Grammatical, but refused because it can mean nothing.
fn semantic_error(query: &str) {
    match parse(query) {
        Err(e) if e.kind == ParseErrorKind::Semantic => {}
        other => panic!("{query}: expected a semantic refusal, got {other:?}"),
    }
}
fn parses(query: &str) {
    parse(query).unwrap_or_else(|e| panic!("{query}: {e}"));
}

#[test]
fn member_fields_and_filters_accept_grammar_whitespace_and_comments() {
    same(
        "^[referencedComponentId,mapTarget]200001",
        "^ [ referencedComponentId , mapTarget ] 200001",
    );
    same("^[*]200001", "memberOf [ any ] 200001");
    same("^[*]200001", "^\t[\r\n*\n]/* fields */200001");
    same(
        "^[mapTarget]200001 {{M mapGroup=#1}}",
        "^ [mapTarget] 200001 {{ /*a*/ M /*b*/ mapGroup /*c*/ = /*d*/ #1 /*e*/ }}",
    );
    same(
        "^200001 {{M mapGroup=#1, mapTarget=\"x\"}}",
        "^200001{{M mapGroup=#1,mapTarget=\"x\"}}",
    );
    same(
        "^200001 {{M mapGroup=#1}} {{M active=1}}",
        "^200001 {{M mapGroup=#1}}{{M active=1}}",
    );
    for invalid in [
        "^[]200001",
        "^[ ]200001",
        "^[mapTarget1]200001",
        "^[map-Target]200001",
        "^[map Target]200001",
        "^[mapTarget,]200001",
        "^[mapTarget]",
        "^R[mapTarget]200001",
        "^ R 200001",
        "^ r 200001",
        "^r[mapTarget]200001",
        "^200001 {{M}}",
        "^200001 {{M mapGroup=#1,}}",
        "^200001 {{M mapGroup=#1 mapTarget=\"x\"}}",
        "^200001 {{M map1Group=#1}}",
        "^200001 {{ M mapGroup=#1 }",
        "^200001 {{M mapGroup==#1}}",
        // Member filters precede concept and description filters in subExpressionConstraint.
        "^200001 {{C active=1}} {{M active=1}}",
        "^200001 {{D active=1}} {{M active=1}}",
    ] {
        syntax_error(invalid);
    }
}

#[test]
fn member_field_values_follow_numeric_time_string_and_boolean_lexemes() {
    for valid in [
        "^200001 {{M mapGroup=#0}}",
        "^200001 {{M mapGroup=#0.0}}",
        "^200001 {{M mapGroup=#-0.5}}",
        "^200001 {{M mapGroup=#+3}}",
        "^200001 {{M mapGroup>=#123456789012345678901234567890.5}}",
        "^200001 {{M mapGroup not = #1}}",
        "^200001 {{M mapGroup <> #1}}",
        "^200001 {{M mapGroup != #1}}",
        "^200001 {{M effectiveTime=\"\"}}",
        "^200001 {{M effectiveTime=\"20260826\"}}",
        "^200001 {{M effectiveTime=(\"20260826\" \"\")}}",
        "^200001 {{M effectiveTime <= \"20240229\"}}",
        "^200001 {{M reviewDate>\"20260826\"}}",
        "^200001 {{M mapTarget=\"J45.9\"}}",
        "^200001 {{M mapTarget=(\"J45.9\" \"J46\")}}",
        "^200001 {{M mapTarget=wild:\"J45*\"}}",
        "^200001 {{M mapTarget=match:\"J45 X\"}}",
        "^200001 {{M mapTarget=(\"a\" wild:\"b*\" match:\"c\")}}",
        "^200001 {{M mapTarget=\"a\\\"b\"}}",
        "^200001 {{M mapTarget=\"a\\\\b\"}}",
        "^200001 {{M mapTarget=wild:\"a\\*b\"}}",
        "^200001 {{M mapTarget=\"caf\u{e9} \u{1f600}\"}}",
        "^200001 {{M grouped=true}}",
        "^200001 {{M grouped = FALSE}}",
        "^200001 {{M grouped != true}}",
        "^200001 {{M active=1}}",
        "^200001 {{M active=TRUE}}",
        "^200001 {{M active=0}}",
        "^200001 {{M active=false}}",
        "^200001 {{M active=*}}",
        "^200001 {{M active=\"*\"}}",
        "^200001 {{M active != 1}}",
        "^200001 {{M moduleId=<<900000000000443000}}",
        "^200001 {{M moduleId=(900000000000207008 900000000000012004)}}",
        "^200001 {{M referencedComponentId=(<<1000001 MINUS 1000002)}}",
        "^200001 {{M targetComponentId=*}}",
    ] {
        parses(valid);
    }
    for invalid in [
        "^200001 {{M mapGroup=#1.}}",
        "^200001 {{M mapGroup=#.5}}",
        "^200001 {{M mapGroup=#01}}",
        "^200001 {{M mapGroup=#1e5}}",
        "^200001 {{M mapGroup=#}}",
        "^200001 {{M mapGroup=# 1}}",
        "^200001 {{M mapGroup=1}}",
        "^200001 {{M effectiveTime=\"2026-08-26\"}}",
        "^200001 {{M effectiveTime=\"20261301\"}}",
        "^200001 {{M effectiveTime=(\"20260826\",\"\")}}",
        "^200001 {{M effectiveTime=()}}",
        "^200001 {{M mapTarget=\"a\u{1}b\"}}",
        "^200001 {{M mapTarget=match:\"a\\*b\"}}",
        "^200001 {{M mapTarget=\"a\\nb\"}}",
        "^200001 {{M mapTarget>\"a\"}}",
        "^200001 {{M mapTarget=wild:\"\"}}",
        "^200001 {{M mapTarget=(\"a\",\"b\")}}",
        "^200001 {{M grouped=yes}}",
        "^200001 {{M grouped>true}}",
        "^200001 {{M active=2}}",
        "^200001 {{M active=\"1\"}}",
        "^200001 {{M active<1}}",
        "^200001 {{M moduleId=}}",
    ] {
        syntax_error(invalid);
    }
    // Member filters are defined over memberOf rows; the grammar's optional operator is a
    // semantic error, not a syntax error, wherever the filter appears.
    for undefined in [
        "200001 {{M active=1}}",
        "<< 200001 {{M mapGroup=#1}}",
        "(200001 OR 200002) {{M active=1}}",
        "* {{M active=1}}",
        "^200001 AND (200002 {{M active=1}})",
        "* : 1000001 = (200001 {{M active=1}})",
    ] {
        match parse(undefined) {
            Err(e) if e.kind == ParseErrorKind::Semantic => {}
            other => panic!("{undefined}: expected a semantic error, got {other:?}"),
        }
    }
    // The grammar's generic field filter also parses these; their types fail at evaluation.
    for typed_at_evaluation in [
        "^200001 {{M active=#1}}",
        "^200001 {{M active=10000001}}",
        "^200001 {{M active=<<1000001}}",
        "^200001 {{M effectiveTime=#20260826}}",
        "^200001 {{M moduleId=\"core\"}}",
        "^200001 {{M effectiveTime=true}}",
        // An empty quoted value is only a timeValue; a string column rejects it at evaluation.
        "^200001 {{M mapTarget=\"\"}}",
    ] {
        parses(typed_at_evaluation);
    }
}

#[test]
fn concept_references_terms_and_identifiers_follow_their_lexemes() {
    same("1000001", "1000001 |a  b  c|");
    same("1000001", "1000001 |\u{e9}\u{1f600}|");
    same("1000001", "1000001 | spaced |");
    same("1000001", "1000001|adjacent|");
    same("1000001", "1000001 |{{ not a filter }}|");
    same("1000001", "1000001 |a/*b*/c|");
    for valid in [
        "100000",
        "123456789012345678",
        "\"demo#a b\"",
        "demo#a.b-c_D",
    ] {
        parses(valid);
    }
    for invalid in [
        "10000",
        "1234567890123456789",
        "0100000",
        "1000001 |a\tb|",
        "1000001 |a\nb|",
        "1000001 |a|b|",
        "1000001 ||",
        "1000001 |a| |b|",
        "demo#a b",
        "de mo#a",
        "1demo#a",
        "demo#",
        "\"demo#a\\\"b\"",
        "demo#a|term",
    ] {
        syntax_error(invalid);
    }
    assert_eq!(
        parse("demo-2#A.1 |term|").unwrap(),
        Expr::AlternateIdentifier {
            scheme: "demo-2".into(),
            code: "A.1".into()
        }
    );
}

#[test]
fn operators_keywords_cardinalities_and_comments_follow_whitespace_rules() {
    same("<<1000001", "<< 1000001");
    same("<<1000001", "descendantOrSelfOf\t1000001");
    same("<<1000001", "DESCENDANTORSELFOF/**/1000001");
    same("!!>1000001", "TOP 1000001");
    same("!!<(1000001 OR 1000002)", "bottom (1000001 OR 1000002)");
    same("1000001 AND 1000002", "1000001\r\nAND\r\n1000002");
    same("1000001 AND 1000002", "1000001 AND/* comment */1000002");
    same("1000001 OR 1000002", "1000001 or 1000002");
    same("1000001 MINUS 1000002", "1000001 minus 1000002");
    same("* : [0..*] 1000001 = *", "ANY : [0 to many] 1000001 = ANY");
    same("* : [1..1] 1000001 = *", "* : [1 to 1] 1000001 = *");
    same("* : 1000001 != *", "* : 1000001 not = *");
    same("* : 1000001 != *", "* : 1000001 NOT= *");
    same("* : 1000001 != *", "* : 1000001 <> *");
    same("* : R 1000001 = *", "* : reverseOf 1000001 = *");
    // ABNF terminals are case-insensitive (RFC 5234 2.3) and ECL.g4 writes (CAP_R | R).
    same("* : R 1000001 = *", "* : r 1000001 = *");
    same("* : R 1000001 = *", "* : r1000001 = *");
    same("* : R 1000001 = *", "* : R1000001 = *");
    same("* : R < 1000001 = *", "* : RdescendantOf 1000001 = *");
    same("* : R < 1000001 = *", "* : rdescendantOf 1000001 = *");
    same(
        "* : R < 1000001 = *",
        "* : reverseOfdescendantOf 1000001 = *",
    );
    same(
        "* : R ^R 1000001 = *",
        "* : RrefsetContainingAny 1000001 = *",
    );
    same("* : R 1000001 = *", "* : REVERSEOF 1000001 = *");
    same("* : R 1000001 = *", "* : reverseOf1000001 = *");
    same("* : R (1000001) = *", "* : r(1000001) = *");
    same("^R 1000001", "^r 1000001");
    same("^R 1000001", "refsetContainingAny 1000001");
    same("^R (1000001)", "REFSETCONTAININGANY(1000001)");
    same("* : R ^R 1000001 = *", "* : r ^r 1000001 = *");
    // A single r is a flag; longer r-initial words are attribute names or aliases.
    same("* : refsetContainingAny 1000001 = *", "* : ^R 1000001 = *");
    assert_eq!(
        parse("* : r#A = *").unwrap(),
        parse("* : \"r#A\" = *").unwrap()
    );
    assert!(matches!(
        parse("* : r r#A = *").unwrap(),
        Expr::Refined(_, refinement)
            if matches!(*refinement, snomed_ecl_engine::ecl::Refinement::Attribute(ref a)
                if a.reverse && matches!(*a.name, Expr::AlternateIdentifier { .. }))
    ));
    same("*", "/* * / */ *");
    same("*", "/**/*/**/");
    same("*", "/* \u{e9} */ *");
    for invalid in [
        "<< 1000001 AND",
        "1000001 AND1000002",
        "1000001 ANDOR 1000002",
        "descendantOrSelfOf1000001",
        "top1000001",
        "* : [ 0..1 ] 1000001 = *",
        "* : [0 .. 1] 1000001 = *",
        "* : [0..1 ] 1000001 = *",
        "* : [0to1] 1000001 = *",
        "* : [0..] 1000001 = *",
        "* : [..1] 1000001 = *",
        "* : [00..1] 1000001 = *",
        "* : 1000001 = = *",
        "* : 1000001 not= = *",
        "* : 1000001 =! *",
        "* : 1000001 < > *",
        "* : reverse 1000001 = *",
        "* : rr 1000001 = *",
        "* : R R 1000001 = *",
        "/* a /* b */ c */ *",
        "/* unclosed * / *",
        "*/ *",
        "* /",
        "<<<1000001",
        "<<!!1000001",
        "!!>>1000001",
        "!!> !!> 1000001",
        "1000001 : : 1000002 = *",
        "^^1000001",
    ] {
        syntax_error(invalid);
    }
}

#[test]
fn filter_and_history_keywords_follow_case_and_delimiter_rules() {
    same("* {{ term = \"a\" }}", "* {{D term=\"a\"}}");
    same("* {{ term = \"a\" }}", "* {{ d TERM = \"a\" }}");
    same(
        "* {{C definitionStatus = primitive}}",
        "* {{ c DEFINITIONSTATUS = PRIMITIVE }}",
    );
    same(
        "* {{C definitionStatus = (primitive defined)}}",
        "* {{C definitionStatus = ( primitive\tdefined )}}",
    );
    same("* {{C moduleId = 1000001}}", "* {{C moduleId = (1000001)}}");
    // A parenthesised conjunction is a subExpressionConstraint, not a reference set.
    same(
        "* {{C moduleId = (1000001 AND 1000002)}}",
        "* {{C moduleId = (1000001, 1000002)}}",
    );
    same(
        "* {{C moduleId = (1000001 OR 1000002)}}",
        "* {{C moduleId = (1000001 1000002)}}",
    );
    same("* {{ +HISTORY-MIN }}", "* {{+history_min}}");
    same("* {{ +HISTORY-MOD }}", "* {{ + History-Mod }}");
    same("* {{ +HISTORY }}", "* {{+HISTORY}}");
    same(
        "* {{ +HISTORY (< 1000001) }}",
        "* {{ + history ( <1000001 ) }}",
    );
    same(
        "* {{ dialect = en-gb (prefer) }}",
        "* {{ D dialect = en-gb ( PREFER ) }}",
    );
    same(
        "* {{ type = (syn fsn def) }}",
        "* {{ type = (SYNONYM FullySpecifiedName Definition) }}",
    );
    same(
        "* {{ dialect = en-gb (accept) }}",
        "* {{ dialect = en-gb (ACCEPTABLE) }}",
    );
    same("* {{C active = *}}", "* {{C active = any}}");
    same(
        "* {{ language = (en fr) }}",
        "* {{ D LANGUAGE = ( EN FR ) }}",
    );
    for invalid in [
        "* {{C moduleId = ()}}",
        "* {{C definitionStatus = (primitive, defined)}}",
        "* {{C definitionStatus = fullyDefined}}",
        "* {{C active = }}",
        "* {{C effectiveTime = 20260826}}",
        "* {{C term = \"a\"}}",
        "* {{D definitionStatus = primitive}}",
        "* {{ +HISTORY-MIN (< 1000001) }}",
        "* {{ +HISTORY-MAXIMUM }}",
        "* {{ +HISTORY-MIN}",
        "* {{ + }}",
        "* {{ language = e }}",
        "* {{ language = eng }}",
        "* {{ language = (en, fr) }}",
        "* {{ dialect = en_gb }}",
        "* {{ dialect = 900000000000508004 }}",
        "* {{ type = (syn, fsn) }}",
        "* {{ id = 12345 }}",
        "* {{ id = \"123456\" }}",
        "* {{ }}",
        "^* {{ M }}",
        "^* {{ M active }}",
        "* {{",
        "* }}",
    ] {
        syntax_error(invalid);
    }
}

#[test]
fn grammatical_values_that_name_nothing_are_refused_after_parsing() {
    // The grammar allows day 31 in any month and any pair of cardinality bounds.
    semantic_error("^200001 {{M effectiveTime=\"20260231\"}}");
    semantic_error("^200001 {{M effectiveTime=\"20230229\"}}");
    semantic_error("< 1000001 {{C effectiveTime = \"20260931\"}}");
    semantic_error("* : [1..0] 1000001 = *");
    // Malformed text around a refusal is still a syntax error.
    syntax_error("* : [1..0] 1000001 = * )");
    syntax_error("^200001 {{M effectiveTime=\"20260231\"} }");
    syntax_error("1000001 {{M active=1}} (");
}

#[test]
fn long_syntax_keywords_may_run_into_not_and_boolean_operators() {
    same("< 1000001 {{D id NOT = 1000002}}", "< 1000001 {{D idNOT= 1000002}}");
    same("< 1000001 {{C active NOT = 0}}", "< 1000001 {{C activeNot=0}}");
    same("* : 1000001 = true OR 1000002 = *", "* : 1000001 = trueOR 1000002 = *");
    // A field that merely ends in `not` stays a field.
    parses("^200001 {{M cannot = 1000001}}");
}

#[test]
fn a_dot_ends_an_alternate_code_only_before_an_attribute() {
    same("x#a. 1000001", "x#a . 1000001");
    same("x#a..1000001", "x#a. . 1000001");
    parses("x#a.b");
    parses("x#1.2");
}
