"""Test vectors for the XML parser and serializer of fontoxide-subset's SVG module.

Writes one case per line: the document and the results of `etree.tostring` without
and with `pretty_print`, in hexadecimal and separated by tabs, with `!` for a syntax
error. Compare with:

    uv run python lxml_vectors.py <output> [seed] [count]
    FONTOXIDE_LXML_VECTORS=<output> cargo test -p fontoxide-subset lxml_vectors -- --ignored
"""

import random
import sys

from lxml import etree

FRAGMENTS = [
    " ", "  ", "\n", "\t", "\r\n", "\r", "text", "x y", "中", "\U0001f600", "\u0085",
    "]", "]]", "&lt;", "&gt;", "&amp;", "&quot;", "&apos;", "&#32;", "&#x20;", "&#10;",
    "&#13;", "&#9;", "&#20013;", "&e;",
    "<![CDATA[]]>", "<![CDATA[ ]]>", "<![CDATA[<x>&\r\n]]>",
    "<!--c-->", "<!-- -->", "<!---->", "<!--中\r\n-->", "<?p?>", "<?p ?>", "<?p q  r ?>",
    "<?p 中\r\n?>",
    "<b/>", "<b></b>", "<b> </b>", "<b>x</b>", "<b>\n</b>", "<c:d/>", "<i xmlns=\"\"/>",
    "<j xmlns=\"u:j\"/>", "<k xml:space=\"preserve\"> <l/> </k>",
    "<k xml:space=\"default\"> <l/> </k>", "<m a=\"1\" c:b=\"2\"/>", "<n xml:lang=\"zh\"/>",
    "<o xmlns:p=\"u:p\" p:q=\"1\"><p:r/></o>",
]
ATTRS = [
    " a=\"1\"", " a='1'", " a = \"1\"", "\ta=\"1\"\n", " a=\"&#10;\n\t\r\n x\"", " a=\"&lt;&gt;&amp;&quot;&apos;\"",
    " a='\"'", " a=\"'\"", " a=\"中\"", " a=\"\U0001f600\"", " a=\"\u0085\"", " b=\"2\" a=\"1\"",
    " xmlns=\"u:d\"", " xmlns=\"\"", " xmlns:z=\"u:z\" z:y=\"1\"", " z:y=\"1\" xmlns:z=\"u:z\"",
    " xml:space=\"preserve\"", " xml:space=\"default\"", " xml:space=\"other\"",
    " xmlns:c=\"u:other\"", " a=\"]]>\"", " a=\" x \"", " a=\"\"",
]
PREFIXES = [
    "", "", "", "<?xml version=\"1.0\"?>", "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    "<?xml version='1.1' encoding='utf8' standalone='yes'?>", "﻿", "\n ",
    "<!-- before -->", "<?p q?>", "<!DOCTYPE a>", "<!DOCTYPE a [ ]>\n",
]
BAD = ["<", "&", ">", "/", "<!", "<?", "]]>", "&#0;", "&#xFFFE;", "\x01", "<a", "</", "--", "=", "\"", "&bad;", "<1/>", "<a:b:c/>"]
HAND = [
    "<a xmlns:x=\"中\"/>", "<a xmlns:x=\"a b\"/>", "<a xmlns:x=\"a[b\"/>", "<a xmlns:x=\"a#b#c\"/>",
    "<a xmlns:x=\"%zz\"/>", "<a xmlns:x=\"%41\"/>", "<a xmlns:x=\"#\"/>", "<a xmlns:x=\"a:b:c\"/>",
    "<a xmlns:x=\"a&amp;b\"/>", "<a xmlns:x=\"a'b\"/>", "<a xmlns:x='a&apos;b'/>",
    "<a xmlns:x=\"http://x/y?z=1#f\"/>", "<a xmlns=\"u\"><x xmlns=\"\"><y xmlns=\"u\"/></x></a>",
    "<a xmlns:x=\"u\"><x:b xmlns:x=\"v\"/></a>", "<xml/>", "<xmlns/>", "<a xml:b=\"1\"/>",
    "<a><?xml-x y?></a>", "<a><?Xml?></a>", "<a><!DOCTYPE b></a>", "<!DOCTYPE a><!DOCTYPE a><a/>",
    "<a/><!DOCTYPE a>", "<!DOCTYPE a [<!ENTITY e \"v\">]><a>&e;</a>",
    "<!DOCTYPE a [<!-- c --><?p q?><!ENTITY e 'v'>]><a> &e; <b/> </a>",
    " " * 400 + "<a/>", "<a>" + " " * 400 + "<b/></a>", "<a>中" + " " * 700 + "<b/>" + " " * 3 + "<c/></a>",
    "<a>" + "x" * 700 + "</a>", "<a>\r" + " " * 400 + "<b/></a>",
    "<a><b><c><d><e><f><g><h/></g></f></e></d></c></b></a>",
    "<a xml:space=\"preserve\"><b/></a>", "<a><b xml:space=\"preserve\"><c/></b></a>",
    "<a xml:space=\"preserve\"><b> <c/> </b></a>",
    "".join(f"<e{i}>" for i in range(70)) + "<x/>" + "".join(f"</e{i}>" for i in reversed(range(70))),
]


def element(rng, depth):
    name = rng.choice(["a", "a", "svg", "c:d", "中", "a-b.c_1"])
    attrs = "".join(rng.choice(ATTRS) for _ in range(rng.choice([0, 0, 1, 1, 2])))
    decl = " xmlns:c=\"u:c\"" if depth == 0 else ""
    if rng.random() < 0.1:
        return f"<{name}{decl}{attrs}/>"
    content = []
    for _ in range(rng.randint(0, 6)):
        if depth < 3 and rng.random() < 0.3:
            content.append(element(rng, depth + 1))
        else:
            content.append(rng.choice(FRAGMENTS))
    return f"<{name}{decl}{attrs}>{''.join(content)}</{name}>"


def document(rng):
    doc = rng.choice(PREFIXES) + element(rng, 0)
    if "&e;" in doc and rng.random() < 0.8:
        doc = "<!DOCTYPE a [<!ENTITY e \"v\">]>" + doc.lstrip("﻿")
    if rng.random() < 0.1:
        doc += rng.choice(["\n", " ", "<!-- after -->", "<?p?>"])
    if rng.random() < 0.08:
        i = rng.randint(0, len(doc))
        doc = doc[:i] + rng.choice(BAD) + doc[i:]
    return doc


def result(doc, pretty):
    parser = etree.XMLParser(huge_tree=True, remove_blank_text=True, resolve_entities=False)
    try:
        tree = etree.fromstring(doc.encode("utf-8"), parser=parser)
        return etree.tostring(tree, pretty_print=pretty).hex()
    except etree.XMLSyntaxError:
        return "!"


def main():
    output = sys.argv[1]
    rng = random.Random(int(sys.argv[2]) if len(sys.argv) > 2 else 0)
    count = int(sys.argv[3]) if len(sys.argv) > 3 else 20000
    docs = HAND + [document(rng) for _ in range(count)]
    with open(output, "w", encoding="ascii", newline="\n") as f:
        for doc in docs:
            hexdoc = doc.encode("utf-8").hex()
            f.write(f"{hexdoc}\t{result(doc, False)}\t{result(doc, True)}\n")


if __name__ == "__main__":
    main()
