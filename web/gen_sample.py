#!/usr/bin/env -S uv run --with reportlab --script
"""Generate web/public/sample.pdf: a small public-domain test doc (Alice opening)
with a wrapped 'dai- sies', an 8pt footnote, and page numbers — exercises the
clean pipeline (body-size filter, dehyphenation, page-number drop)."""
from reportlab.lib.pagesizes import letter
from reportlab.lib.styles import getSampleStyleSheet, ParagraphStyle
from reportlab.platypus import SimpleDocTemplate, Paragraph, Spacer
from reportlab.lib.units import inch
S = getSampleStyleSheet()
body = ParagraphStyle("body", parent=S["BodyText"], fontSize=12, leading=16, firstLineIndent=18, spaceAfter=0)
foot = ParagraphStyle("foot", parent=S["BodyText"], fontSize=8, leading=10, textColor="grey")
def pn(c, d):
    c.saveState(); c.setFont("Times-Roman", 9); c.drawCentredString(letter[0]/2, 0.5*inch, str(d.page)); c.restoreState()
T = [
 "Alice was beginning to get very tired of sitting by her sister on the bank, and of having nothing to do.",
 "She was considering whether the pleasure of making a daisy-chain would be worth the trouble of getting up and picking the dai- sies, when suddenly a White Rabbit ran close by her.",
 "There was nothing so very remarkable in that; nor did Alice think it so very much out of the way to hear the Rabbit say, Oh dear! I shall be late!",
]
doc = SimpleDocTemplate("public/sample.pdf", pagesize=letter, topMargin=inch, bottomMargin=inch)
flow = [Paragraph("Down the Rabbit-Hole", S["Title"])]
for t in T: flow += [Paragraph(t, body), Spacer(1, 6)]
flow += [Spacer(1, 12), Paragraph("1. A footnote that should be dropped by the body-size filter.", foot)]
doc.build(flow, onFirstPage=pn, onLaterPages=pn)
print("wrote public/sample.pdf")
