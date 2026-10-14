"""Build the ShannonsPrism logos in docs/assets from the brand spec: light and dark lockups
and the app icon, each as a 2x PNG (via headless Firefox) and an SVG with the fonts embedded.
Fonts: Space Grotesk and IBM Plex Mono, latin subsets, SIL Open Font License
(OFL-SpaceGrotesk.txt, OFL-IBMPlex.txt). Run: python3 docs/brand/build_logos.py
"""
import base64, os, subprocess, tempfile
from PIL import ImageFont
HERE=os.path.dirname(os.path.abspath(__file__))
OUT=os.path.join(os.path.dirname(HERE), "assets")
b64=lambda p: base64.b64encode(open(os.path.join(HERE,p),"rb").read()).decode()
SG=b64("SpaceGrotesk.woff2"); PM=b64("IBMPlexMono-400.woff2")
FONTCSS=f"""@font-face{{font-family:'Space Grotesk';font-weight:500 600;src:url(data:font/woff2;base64,{SG}) format('woff2')}}
@font-face{{font-family:'IBM Plex Mono';font-weight:400;src:url(data:font/woff2;base64,{PM}) format('woff2')}}"""
def mark(ink, accent):
    rects=[(8,77,4,6),(14,79,6,4),(22,76,3,8),(28,78,5,5),(36,77,2,7),(41,79,7,4),(51,76,3,6),(57,80,4,4),(63,77,5,6)]
    s="".join(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="1" fill="{ink}"/>' for x,y,w,h in rects)
    s+=f'<line x1="72.7" y1="80" x2="122.7" y2="70" stroke="{accent}" stroke-width="1.5" opacity="0.45"/><line x1="72.7" y1="80" x2="129.5" y2="85" stroke="{accent}" stroke-width="1.5" opacity="0.45"/><line x1="72.7" y1="80" x2="136.4" y2="100" stroke="{accent}" stroke-width="1.5" opacity="0.45"/>'
    s+=f'<polygon points="100,20 150,130 50,130" fill="{ink}" fill-opacity="0.06" stroke="{ink}" stroke-width="5" stroke-linejoin="round"/>'
    s+=f'<line x1="124" y1="69" x2="194" y2="40" stroke="{accent}" stroke-width="6" stroke-linecap="round"/><line x1="131" y1="85" x2="194" y2="85" stroke="{accent}" stroke-width="6" stroke-linecap="round" stroke-dasharray="12 7"/><line x1="138" y1="101" x2="194" y2="130" stroke="{accent}" stroke-width="6" stroke-linecap="round" stroke-dasharray="0.1 9"/>'
    return s
TAG="H(x) = the right compressor, per section"
def lockup_html(bg, ink, accent, tagc):
    return f"""<!doctype html><html><head><meta charset="utf-8"><style>{FONTCSS}
html,body{{margin:0;background:{bg}}} .w{{width:1200px;height:600px;transform:scale(2);transform-origin:0 0;background:{bg};display:flex;align-items:center;justify-content:center;gap:48px;font-family:'Space Grotesk','Helvetica Neue',sans-serif}}</style></head><body><div class="w">
<svg width="300" height="225" viewBox="0 0 200 150" xmlns="http://www.w3.org/2000/svg">{mark(ink,accent)}</svg>
<div style="display:flex;flex-direction:column;gap:14px"><div style="font-size:104px;font-weight:600;letter-spacing:-3px;line-height:1;color:{ink}">Shannons<span style="color:{accent}">Prism</span></div>
<div style="font-family:'IBM Plex Mono','Courier New',monospace;font-size:22px;color:{tagc};letter-spacing:1px">{TAG}</div></div></div></body></html>"""
def icon_html():
    return f"""<!doctype html><html><head><meta charset="utf-8"><style>html,body{{margin:0;background:#16161D}} .w{{width:600px;height:600px;transform:scale(2);transform-origin:0 0;border-radius:140px;background:#16161D;display:flex;align-items:center;justify-content:center}}</style></head><body><div class="w"><svg width="464" height="348" viewBox="0 0 200 150" xmlns="http://www.w3.org/2000/svg">{mark('#F4F1EA','#F0555E')}</svg></div></body></html>"""
def shot(html, name, w, h):
    p=os.path.join(tempfile.gettempdir(), "sprism-"+name+".html"); open(p,"w").write(html)
    prof=os.path.join("/tmp", "sprism-ffprof"); os.makedirs(prof, exist_ok=True)
    out=os.path.join(OUT, name+".png")
    subprocess.run(["firefox","--headless","--no-remote","--profile",prof,f"--window-size={w},{h}","--screenshot",out,"file://"+p],check=True,capture_output=True,timeout=120)
    print("wrote", out)
shot(lockup_html("#F4F1EA","#16161D","#C62835","#55534D"), "logo-light", 2400, 1200)
shot(lockup_html("#121218","#F4F1EA","#F0555E","#B3B0A8"), "logo-dark", 2400, 1200)
shot(icon_html(), "icon", 1200, 1200)

# SVG lockups with embedded fonts, laid out to match the flex centering
sg=ImageFont.truetype(os.path.join(HERE,"SpaceGrotesk.woff2"),104)
try: sg.set_variation_by_axes([600])
except Exception as e: print("var axis:", e)
pm=ImageFont.truetype(os.path.join(HERE,"IBMPlexMono-400.woff2"),22)
wm=sum(sg.getlength(c) for c in "ShannonsPrism") - 3*12   # letter-spacing -3px applied 13 times in CSS minus trailing; close enough
sh=sum(sg.getlength(c) for c in "Shannons") - 3*8
tw=pm.getlength(TAG)+1*len(TAG)
textw=max(wm,tw); total=300+48+textw; x0=(1200-total)/2
blockh=104+14+22*1.3; ytop=(600-blockh)/2
def lockup_svg(bg, ink, accent, tagc):
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="600" viewBox="0 0 1200 600" role="img" aria-label="ShannonsPrism">
<style>{FONTCSS}</style>
<rect width="1200" height="600" fill="{bg}"/>
<svg x="{x0:.1f}" y="{(600-225)/2:.1f}" width="300" height="225" viewBox="0 0 200 150">{mark(ink,accent)}</svg>
<text x="{x0+348:.1f}" y="{ytop+104*0.78:.1f}" font-family="'Space Grotesk','Helvetica Neue',sans-serif" font-weight="600" font-size="104" letter-spacing="-3" fill="{ink}">Shannons<tspan fill="{accent}">Prism</tspan></text>
<text x="{x0+348:.1f}" y="{ytop+104+14+22*0.8:.1f}" font-family="'IBM Plex Mono','Courier New',monospace" font-size="22" letter-spacing="1" fill="{tagc}">{TAG}</text>
</svg>"""
open(os.path.join(OUT,"logo-light.svg"),"w").write(lockup_svg("#F4F1EA","#16161D","#C62835","#55534D"))
open(os.path.join(OUT,"logo-dark.svg"),"w").write(lockup_svg("#121218","#F4F1EA","#F0555E","#B3B0A8"))
open(os.path.join(OUT,"icon.svg"),"w").write(f"""<svg xmlns="http://www.w3.org/2000/svg" width="600" height="600" viewBox="0 0 600 600" role="img" aria-label="ShannonsPrism icon">
<rect width="600" height="600" rx="140" fill="#16161D"/>
<svg x="68" y="126" width="464" height="348" viewBox="0 0 200 150">{mark('#F4F1EA','#F0555E')}</svg>
</svg>""")
print("svgs written; text widths", round(wm), round(tw))
