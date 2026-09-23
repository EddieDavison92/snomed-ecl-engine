"""Checks the parser against the official ECL grammar, production by production.

Reads the pinned ABNF, generates sentences that cover every alternative,
optional part and repetition count of every production, and mutates them into
near misses. An independent recogniser built from the ABNF alone decides what
the grammar accepts; `examples/parse_lines.rs` reports what the parser does.
Every disagreement is listed, and each production records how much of it the
generated sentences exercised.

    python3 scripts/grammar_differential.py PARSE_LINES_BINARY [--seed N] [--write]

Literals are matched case-insensitively, as RFC 5234 defines quoted strings.
"""
import argparse
import json
import random
import re
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GRAMMARS = ROOT / "references/snomed-expression-constraint-language/syntax"
sys.setrecursionlimit(20000)


# ---------------------------------------------------------------- ABNF model

class Alt:
    def __init__(self, options):
        self.options = options


class Seq:
    def __init__(self, items):
        self.items = items


class Rep:
    def __init__(self, low, high, item):
        self.low, self.high, self.item = low, high, item


class Ref:
    def __init__(self, name):
        self.name = name


class Lit:
    def __init__(self, text):
        self.text = text.encode()


class Range:
    def __init__(self, low, high):
        self.low, self.high = low, high


TOKEN = re.compile(r'\s*(?:(;[^\n]*)|("[^"]*")|(%x[0-9A-Fa-f]+(?:-[0-9A-Fa-f]+|(?:\.[0-9A-Fa-f]+)+)?)|(\d*\*\d*|\d+)|([A-Za-z][A-Za-z0-9-]*)|([()\[\]/]))')


def tokens(text):
    at, out = 0, []
    while at < len(text):
        if text[at:].strip() == "":
            break
        m = TOKEN.match(text, at)
        assert m and m.end() > at, f"cannot read {text[at:at + 30]!r}"
        at = m.end()
        if m.group(1):
            continue
        out.append(next(g for g in m.groups()[1:] if g))
    return out


def parse_rules(path):
    return parse_text(path.read_text(encoding="utf-8-sig"))


def parse_text(text):
    rules = {}
    for line in text.splitlines():
        if not line.strip() or "=" not in line:
            continue
        name, body = line.split("=", 1)
        stream = tokens(body)
        node, rest = alternation(stream)
        assert not rest, (name, rest)
        rules[name.strip()] = node
    return rules


def alternation(stream):
    options = []
    seq, stream = concatenation(stream)
    options.append(seq)
    while stream and stream[0] == "/":
        seq, stream = concatenation(stream[1:])
        options.append(seq)
    return Alt(options), stream


def concatenation(stream):
    items = []
    while stream and stream[0] not in ("/", ")", "]"):
        item, stream = repetition(stream)
        items.append(item)
    return Seq(items), stream


def repetition(stream):
    token = stream[0]
    if re.fullmatch(r"\d*\*\d*|\d+", token):
        stream = stream[1:]
        if "*" in token:
            low, high = token.split("*")
            low, high = int(low or 0), int(high) if high else None
        else:
            low = high = int(token)
        item, stream = element(stream)
        return Rep(low, high, item), stream
    item, stream = element(stream)
    return item, stream


def element(stream):
    token, stream = stream[0], stream[1:]
    if token == "(":
        node, stream = alternation(stream)
        assert stream[0] == ")"
        return node, stream[1:]
    if token == "[":
        node, stream = alternation(stream)
        assert stream[0] == "]"
        return Rep(0, 1, node), stream[1:]
    if token.startswith('"'):
        return Lit(token[1:-1]), stream
    if token.startswith("%x"):
        body = token[2:]
        if "-" in body:
            low, high = body.split("-")
            return Range(int(low, 16), int(high, 16)), stream
        values = [int(v, 16) for v in body.split(".")]
        return Seq([Range(v, v) for v in values]), stream
    return Ref(token), stream


# ---------------------------------------------------------------- recogniser

def conventional(rules):
    """The grammar as the specification reads it: the two comment quirks
    removed, and a filter naming no type read as a description filter.

    `ws` admits comments, and `matchSearchTermSet` uses `ws` inside its quotes,
    so `"a/*b*/c"` holds two words rather than the text between the quotes.
    Search terms here take plain whitespace instead.
    """
    rules = dict(rules)
    rules.update(parse_text("\n".join([
        "matchSearchTermSet = QM wsx matchSearchTerm *(mwsx matchSearchTerm) wsx QM",
        "wsx = *( SP / HTAB / CR / LF )",
        "mwsx = 1*( SP / HTAB / CR / LF )",
    ])))
    return rules


class Recogniser:
    """The set of end positions each node can reach from a start position.

    With `closing_comments`, a comment ends at its first `*/`. The official
    rule pairs every star with the byte after it, so a comment ending `**/`,
    such as `/***/`, never closes; that is a defect in the grammar rather than
    a form anyone intends, and this variant labels the disagreements it causes.

    The same variant applies 6.8: "If the type of a filter constraint is not
    specified ... it is assumed that the constraint is a description
    constraint." The ABNF also reads `{{moduleId = x}}` as the member filter
    `m` on a field `oduleId`, because it allows no space after the type letter;
    here a member filter may not begin with `moduleId` run into its letter.
    """

    def __init__(self, rules, closing_comments=False):
        self.rules = rules
        self.closing_comments = closing_comments

    def accepts(self, rule, data):
        self.data, self.memo = data, {}
        return len(data) in self.ends(Ref(rule), 0)

    def ends(self, node, at):
        data = self.data
        if isinstance(node, Ref) and node.name == "comment" and self.closing_comments:
            if data[at:at + 2] != b"/*":
                return set()
            end = data.find(b"*/", at + 2)
            body = data[at + 2:end]
            ok = end >= 0 and all(b in (0x20, 0x09, 0x0D, 0x0A) or 0x21 <= b <= 0x7E or b >= 0x80 for b in body)
            return {end + 2} if ok else set()
        if (isinstance(node, Ref) and node.name == "memberFilterConstraint"
                and self.closing_comments):
            body = data[at + 2:]
            while True:  # whitespace and comments may sit between {{ and the name
                body = body.lstrip(b" \t\r\n")
                end = body.find(b"*/", 2) if body.startswith(b"/*") else -1
                if end < 0:
                    break
                body = body[end + 2:]
            if data[at:at + 2] == b"{{" and body[:8].lower() == b"moduleid":
                return set()
        if isinstance(node, Ref):
            key = (node.name, at)
            if key not in self.memo:
                self.memo[key] = frozenset()  # no left recursion in this grammar
                self.memo[key] = frozenset(self.ends(self.rules[node.name], at))
            return self.memo[key]
        if isinstance(node, Lit):
            end = at + len(node.text)
            return {end} if data[at:end].lower() == node.text.lower() else set()
        if isinstance(node, Range):
            return {at + 1} if at < len(data) and node.low <= data[at] <= node.high else set()
        if isinstance(node, Alt):
            out = set()
            for option in node.options:
                out |= self.ends(option, at)
            return out
        if isinstance(node, Seq):
            current = {at}
            for item in node.items:
                current = {e for p in current for e in self.ends(item, p)}
                if not current:
                    break
            return current
        if isinstance(node, Rep):
            out = {at} if node.low == 0 else set()
            current, count, seen = {at}, 0, set()
            while current and (node.high is None or count < node.high):
                count += 1
                current = {e for p in current for e in self.ends(node.item, p) if e > p}
                if node.high is None:
                    current -= seen
                    seen |= current
                if count >= node.low:
                    out |= current
            return out
        raise TypeError(node)


# ---------------------------------------------------------------- generator

# Real identifiers where the grammar asks for one, so a sentence also means
# something; the check digit is not part of the grammar.
SCTIDS = ["404684003", "195967001", "116680003", "363698007", "900000000000508004", "73211009"]


class Generator:
    def __init__(self, rules, rng):
        self.rules, self.rng = rules, rng
        self.cost = {}
        self.choices = {}  # id(node) -> (production, description)
        self.seen = Counter()
        self.reached = Counter()
        self.label(rules)
        self.minimum()

    def label(self, rules):
        for name, node in rules.items():
            self.walk(name, node, "")

    def walk(self, production, node, path):
        if isinstance(node, Alt) and len(node.options) > 1:
            for i, option in enumerate(node.options):
                self.choices[(id(node), i)] = (production, f"{path}alternative {i + 1}")
        if isinstance(node, Rep):
            buckets = [b for b in (0, 1, 2) if b >= node.low and (node.high is None or b <= node.high)]
            if len(buckets) > 1:
                for b in buckets:
                    self.choices[(id(node), f"n{b}")] = (production, f"{path}{'none' if b == 0 else 'one' if b == 1 else 'several'}")
        children = node.options if isinstance(node, Alt) else node.items if isinstance(node, Seq) else [node.item] if isinstance(node, Rep) else []
        for i, child in enumerate(children):
            self.walk(production, child, f"{path}{i}.")

    def minimum(self):
        """Cheapest derivation size of every node, to finish sentences when deep."""
        cost = {name: 10**9 for name in self.rules}
        changed = True
        while changed:
            changed = False
            for name, node in self.rules.items():
                c = self.size(node, cost)
                if c < cost[name]:
                    cost[name], changed = c, True
        self.cost = cost

    def size(self, node, cost):
        if isinstance(node, Ref):
            return cost[node.name] + 1
        if isinstance(node, Lit):
            return len(node.text)
        if isinstance(node, Range):
            return 1
        if isinstance(node, Alt):
            return min(self.size(o, cost) for o in node.options)
        if isinstance(node, Seq):
            return sum(self.size(i, cost) for i in node.items)
        if isinstance(node, Rep):
            return node.low * self.size(node.item, cost) if node.low else 0

    def sentence(self, rule, budget=40):
        out = bytearray()
        self.emit(Ref(rule), out, budget)
        return bytes(out)

    def pick(self, key_options, deep):
        """Prefers choices not yet seen; when deep, the cheapest."""
        if deep:
            return min(key_options, key=lambda ko: ko[2])
        fresh = [ko for ko in key_options if self.seen[ko[0]] == 0]
        if fresh and self.rng.random() < 0.8:
            return self.rng.choice(fresh)
        return self.rng.choice(key_options)

    def emit(self, node, out, budget):
        deep = budget <= 0
        if isinstance(node, Ref):
            self.reached[node.name] += 1
            if node.name == "sctId" and self.rng.random() < 0.9:
                out += self.rng.choice(SCTIDS).encode()
                return
            if node.name in ("ws", "mws") and self.rng.random() < 0.85:
                out += b"" if node.name == "ws" and self.rng.random() < 0.5 else b" "
                return
            self.emit(self.rules[node.name], out, budget - 1)
        elif isinstance(node, Lit):
            out += node.text
        elif isinstance(node, Range):
            low, high = node.low, node.high
            printable = [b for b in range(max(low, 0x21), min(high, 0x7E) + 1)]
            out.append(self.rng.choice(printable) if printable and self.rng.random() < 0.9 else self.rng.randint(low, high))
        elif isinstance(node, Alt):
            if len(node.options) == 1:
                self.emit(node.options[0], out, budget)
                return
            options = [((id(node), i), o, self.size(o, self.cost)) for i, o in enumerate(node.options)]
            key, option, _ = self.pick(options, deep)
            self.seen[key] += 1
            self.emit(option, out, budget - 1)
        elif isinstance(node, Seq):
            for item in node.items:
                self.emit(item, out, budget)
        elif isinstance(node, Rep):
            buckets = [b for b in (0, 1, 2) if b >= node.low and (node.high is None or b <= node.high)]
            if len(buckets) > 1:
                options = [((id(node), f"n{b}"), b, b) for b in buckets]
                key, count, _ = self.pick(options, deep)
                self.seen[key] += 1
            else:
                count = node.low
            if count == 2 and node.high is None and not deep and self.rng.random() < 0.3:
                count = 3
            count = max(count, node.low)
            for _ in range(count):
                self.emit(node.item, out, budget - 1)


# ---------------------------------------------------------------- the check

def mutations(sentence, rng, alphabet, count):
    out = []
    for _ in range(count):
        data = bytearray(sentence)
        kind = rng.randrange(5)
        at = rng.randrange(len(data) + 1)
        if kind == 0 and data:
            del data[min(at, len(data) - 1)]
        elif kind == 1:
            data.insert(at, rng.choice(alphabet))
        elif kind == 2 and data:
            data[min(at, len(data) - 1)] = rng.choice(alphabet)
        elif kind == 3 and len(data) > 2:
            a = rng.randrange(len(data))
            b = min(len(data), a + rng.randint(1, 8))
            data[b:b] = data[a:b]
        elif kind == 4 and len(data) > 1:
            a = rng.randrange(len(data) - 1)
            data[a], data[a + 1] = data[a + 1], data[a]
        out.append(bytes(data))
    return out


def engine(binary, sentences):
    lines = "".join(json.dumps(list(s)) + "\n" for s in sentences)
    run = subprocess.run([binary], input=lines, capture_output=True, text=True, check=True)
    return [line.split("	") for line in run.stdout.splitlines()]


def shrink(sample, keep, budget=400):
    """Deletes spans while `keep` still holds, for a minimal disagreement."""
    size = max(1, len(sample) // 2)
    while size >= 1 and budget > 0:
        at, changed = 0, False
        while at < len(sample) and budget > 0:
            candidate = sample[:at] + sample[at + size:]
            budget -= 1
            if candidate and keep(candidate):
                sample, changed = candidate, True
            else:
                at += size
        if not changed:
            size //= 2
    return sample


# Engine verdicts that still mean "valid syntax": refused for meaning or size.
GRAMMATICAL = {"ok", "Semantic", "Unsupported", "Limit"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary")
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--sentences", type=int, default=6000)
    parser.add_argument("--mutations", type=int, default=6)
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()
    rng = random.Random(args.seed)
    report = {}
    for name in ("abnf-brief.txt", "abnf-long.txt"):
        rules = parse_rules(GRAMMARS / name)
        recogniser = Recogniser(rules)
        corrected = Recogniser(conventional(rules), closing_comments=True)
        generator = Generator(rules, rng)
        positives = [generator.sentence("expressionConstraint", rng.choice([8, 16, 30])) for _ in range(args.sentences)]
        alphabet = sorted({b for s in positives for b in s} | set(b" \t\r\n()[]{}<>!=^*:,.|\"#-+/\\"))
        negatives = [m for s in positives for m in mutations(s, rng, alphabet, args.mutations)]
        samples = list(dict.fromkeys(positives + negatives))
        verdicts = engine(args.binary, samples)
        disagreements = defaultdict(list)
        engine_kinds = Counter()
        grammar_valid = 0
        for sample, (verdict, *detail) in zip(samples, verdicts):
            valid = recogniser.accepts("expressionConstraint", sample)
            grammar_valid += valid
            engine_kinds[verdict] += 1
            if valid != (verdict in GRAMMATICAL):
                label = "grammar accepts" if valid else "grammar rejects"
                if valid != corrected.accepts("expressionConstraint", sample):
                    label += " only under the literal grammar"
                reason = f"{verdict}: {detail[1]}" if detail else verdict
                disagreements[(label, reason)].append(sample)
        minimal = {}
        for (label, reason), found in disagreements.items():
            def keep(candidate, label=label, reason=reason):
                verdict, *detail = engine(args.binary, [candidate])[0]
                got = f"{verdict}: {detail[1]}" if detail else verdict
                valid = recogniser.accepts("expressionConstraint", candidate)
                literal = valid != corrected.accepts("expressionConstraint", candidate)
                return (got == reason and valid == label.startswith("grammar accepts")
                        and literal == ("literal grammar" in label))
            minimal[(label, reason)] = sorted({shrink(s, keep) for s in sorted(found, key=len)[:3]}, key=len)
        # Every generated positive must itself satisfy the recogniser.
        unrecognised = [s for s in positives if not recogniser.accepts("expressionConstraint", s)]
        coverage = defaultdict(lambda: [0, 0])
        for key, (production, _) in generator.choices.items():
            coverage[production][1] += 1
            coverage[production][0] += generator.seen[key] > 0
        productions = {}
        for production in rules:
            covered, total = coverage.get(production, [0, 0])
            productions[production] = {"choices": total, "covered": covered,
                                       "generated": generator.reached[production]}
        report[name] = {
            "samples": len(samples),
            "grammar_valid": grammar_valid,
            "engine_verdicts": dict(engine_kinds),
            "generator_errors": len(unrecognised),
            "disagreements": {
                f"{a}, engine {b}": {"count": len(v), "minimal": [s.decode("utf-8", "backslashreplace") for s in minimal[(a, b)]]}
                for (a, b), v in sorted(disagreements.items(), key=lambda kv: -len(kv[1]))
            },
            "productions": productions,
            "uncovered": sorted(
                f"{p}: {d}" for key, (p, d) in generator.choices.items() if generator.seen[key] == 0
            ),
        }
        summary = report[name]
        unexplained = sum(d["count"] for k, d in summary["disagreements"].items() if "literal grammar" not in k)
        summary["unexplained"] = unexplained
        print(f"{name}: {summary['samples']} samples, {grammar_valid} grammar-valid, "
              f"{sum(d['count'] for d in summary['disagreements'].values())} disagreements, "
              f"{unexplained} unexplained, "
              f"{len(summary['uncovered'])} uncovered choices, {len(unrecognised)} generator errors")
        for label, detail in summary["disagreements"].items():
            print(f"  {label}: {detail['count']}")
            for example in detail["minimal"]:
                print(f"    {ascii(example)}")
        for item in summary["uncovered"][:20]:
            print(f"  uncovered {item}")
    if args.write:
        path = ROOT / "validation/grammar-differential.json"
        path.write_text(json.dumps({"seed": args.seed, "grammars": report}, indent=1) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
