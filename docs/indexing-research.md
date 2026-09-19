# What Snowstorm's indexes suggest for our engine

Inspected the pinned Snowstorm and Snowstorm Lite commits in [references.json](references.json). These are source findings and proposed experiments, not demonstrated performance gains.

## Existing indexes

Both implementations already precompute hierarchy ancestry. A descendant query finds concepts whose indexed ancestor field contains the requested concept. Our baseline is therefore an indexed search engine, not a graph walker. Replacing traversal with a bitmap is not, by itself, an improvement over Snowstorm.

Full Snowstorm keeps a semantic document with parents, ancestors, flattened attribute values and a stored representation of relationship groups. Its ECL evaluator builds Elasticsearch queries. Branch visibility and stated/inferred selection are part of the query context. The stored `attrMap` preserves the information that the flattened attribute index loses. A declared `refsets` field does not establish that membership evaluation uses it; the inspected membership paths query refset/member data separately. [QueryConcept](https://github.com/IHTSDO/snowstorm/blob/3c10e63893fd434249a9e22b8c165226bda61ee6/src/main/java/org/snomed/snowstorm/core/data/domain/QueryConcept.java#L37)

Lite stores one Lucene document per concept. Parents, ancestors, children and refsets are indexed and stored. Attribute type/target pairs are indexed; complete grouped relationships are separately serialised for retrieval. Descriptions have analysed text fields and a stored display representation. This supports lookup, text search, normal forms and FHIR as well as ECL. [CodeSystemRepository](https://github.com/IHTSDO/snowstorm-lite/blob/6942831706b68d23a028e16e92d23ea31d10653c/src/main/java/org/snomed/snowstormlite/service/CodeSystemRepository.java#L263)

Lucene already compresses stored data and postings. A custom engine must reduce work or choose a better representation for this workload; using Rust and compressed integers alone does not establish an advantage. [Stored fields](https://lucene.apache.org/core/9_12_3/core/org/apache/lucene/codecs/lucene90/Lucene90StoredFieldsFormat.html), [postings format](https://lucene.apache.org/core/9_12_2/core/org/apache/lucene/codecs/lucene912/Lucene912PostingsFormat.html)

## The strongest opportunity: grouped refinements

Full Snowstorm explicitly marks every attribute-group constraint as requiring an inclusion filter. The refined query obtains each candidate's stored `attrMap`, converts it into nested group/type/value maps and checks it in Java. When this filter is present, the selector streams the candidate results, collects matching IDs, sorts them and then applies pagination. This applies to that execution path; caches or other paths can avoid some work. [Group constraints](https://github.com/IHTSDO/snowstorm/blob/3c10e63893fd434249a9e22b8c165226bda61ee6/src/main/java/org/snomed/snowstorm/ecl/domain/refinement/SEclAttributeGroup.java#L31), [matching](https://github.com/IHTSDO/snowstorm/blob/3c10e63893fd434249a9e22b8c165226bda61ee6/src/main/java/org/snomed/snowstorm/ecl/domain/expressionconstraint/SRefinedExpressionConstraint.java#L59), [selection](https://github.com/IHTSDO/snowstorm/blob/3c10e63893fd434249a9e22b8c165226bda61ee6/src/main/java/org/snomed/snowstorm/ecl/ConceptSelectorHelper.java#L122)

Test two representations in Rust:

1. Use concept-level postings to select candidates, then inspect compact numeric relationship rows grouped by concept and group. This keeps Snowstorm's broad strategy while removing textual decoding and nested maps.
2. Assign an internal ID to each eligible `(concept, relationship group)` pair and index attributes to those group IDs. Intersect matching group sets before projecting them back to concepts. This can answer existential same-group conjunctions without fetching every candidate concept.

The second option is a specific indexing experiment, not a promise to support all refinements with bitmaps. Keep group zero separate under the specification's rules. Attribute counts, group counts, missing attributes, inequality, nested disjunctions and concrete comparisons still need exact semantics. Preserve relationship multiplicities and typed values. Do not substitute concept-level intersection for same-group matching.

Measure extra index size against candidate reduction and query latency. Selective queries may favour simply inspecting a few compact relationship rows. Broad grouped queries may justify the extra group index. Lite rejects groups and non-default cardinality, so compare this work against full Snowstorm and version-pinned OneLondon results.

## Other experiments

| Source finding | Rust experiment | What to measure |
|---|---|---|
| Lite expands nontrivial attribute-name/value expressions into ID sets, reading stored documents for IDs before constructing string-valued term queries | Retain dense integer sets throughout evaluation; choose between scanning selected source relationships and reverse type/value postings | Intermediate allocations, stored-data reads and latency for broad target sets |
| Full Snowstorm's exact concept-value check uses `List<String>.contains` | Test bitmap membership against the allowed target set | Relationship checks with broad allowed-value hierarchies |
| Lite retrieves full concept objects for relationship projections | Keep hierarchy, relationships, text and display metadata in separate sections | Bytes read and allocations for ancestors, dotted expressions and ID-only output |
| Full Snowstorm supports branches and both modelling views | Build one immutable inferred edition with an explicit version manifest | Total footprint and query costs under equal output requirements |
| Lite's ancestor recursion revisits already-added ancestors and does not memoise across concepts | Compute validated hierarchy closures in topological order and write the result once | Import CPU and peak RAM, independently of serving speed |
| Lite opens/closes a Lucene writer for each write batch | Build the binary snapshot in bulk | Import wall time and written bytes |

The Lite expansion path is in [ExpressionConstraintLanguageService](https://github.com/IHTSDO/snowstorm-lite/blob/6942831706b68d23a028e16e92d23ea31d10653c/src/main/java/org/snomed/snowstormlite/service/ecl/ExpressionConstraintLanguageService.java#L69). Hierarchy construction is in [FHIRConcept](https://github.com/IHTSDO/snowstorm-lite/blob/6942831706b68d23a028e16e92d23ea31d10653c/src/main/java/org/snomed/snowstormlite/domain/FHIRConcept.java#L205), and batch writes are in [IndexIOProvider](https://github.com/IHTSDO/snowstorm-lite/blob/6942831706b68d23a028e16e92d23ea31d10653c/src/main/java/org/snomed/snowstormlite/service/IndexIOProvider.java#L47). RF2 iteration is partly inside an external dependency; repeated full-archive scans have not been established here.

Full Snowstorm's value check is in [AttributeRange](https://github.com/IHTSDO/snowstorm/blob/3c10e63893fd434249a9e22b8c165226bda61ee6/src/main/java/org/snomed/snowstorm/ecl/domain/refinement/AttributeRange.java#L71). Its [query service](https://github.com/IHTSDO/snowstorm/blob/3c10e63893fd434249a9e22b8c165226bda61ee6/src/main/java/org/snomed/snowstorm/ecl/ECLQueryService.java#L144) caches results by release/branch context, expression, modelling view and page. Measure uncached execution separately from repeated-query cache hits.

## Measurements from our release

The completed Lite index occupies 506,801,604 bytes. Standalone `.fdt` stored-field files account for 288,688,356 bytes, about 57%. Another 109,701,516 bytes are compound segment files, which mix index components. This is not a field-level breakdown: we cannot call the 289 MB "text", or claim it can all be removed. It does show why 507 MB is not a minimum size for numeric ECL indexes.

A separate pass over the ordinary RF2 relationship snapshot found 4,568,005 active rows, all with the inferred characteristic type. Of these, 1,607,583 are `is a` links and 2,960,422 are other attributes across 127 attribute types. There are 1,029,225 attribute rows with a nonzero group. These figures exclude the separate concrete-value relationship file. They count rows, not distinct groups or validated unique tuples.

The local smoke run matched OneLondon on all eight probes. This establishes only that small shared subset on one release. It does not exercise grouped refinements, compare Rust performance or establish serving-memory requirements.

## Next decision

Build the numeric relationship representation first. Compare candidate filtering plus grouped-row scans with an added group-level index. Test small and broad source populations and target hierarchies, including zero-match cases and same-concept/different-group traps. Require exact answers before timing and include index size and resident memory in every comparison.

Keep compressed ancestor postings in the experiment, but do not present precomputed ancestry as a new algorithm. The credible advantage is matching the storage and execution directly to ECL while avoiding unnecessary decoding and service machinery.
