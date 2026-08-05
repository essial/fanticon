import pathlib

base = pathlib.Path(__file__).parent
parts = [
    "frontmatter.html",
    "part1.html",
    "part2.html",
    "part3.html",
    "part4.html",
    "part5.html",
    "part6.html",
    "part7_gamedev.html",
    "appendixA.html",
    "appendixB.html",
    "appendixC.html",
    "appendixD.html",
    "appendixEFG.html",
]

body = "\n".join((base / p).read_text(encoding="utf-8") for p in parts)

html = f"""<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Fanticon Developer's Guide</title>
<link rel="stylesheet" href="style.css">
</head>
<body>
{body}
</body>
</html>
"""

out = base / "fanticon_full.html"
out.write_text(html, encoding="utf-8")
print("wrote", out, len(html), "bytes")
