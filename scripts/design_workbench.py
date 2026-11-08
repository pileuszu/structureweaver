#!/usr/bin/env python3
"""Editable StructureWeaver UI board authored/rendered with the installed DesignCraft CLI.
Run: python3 scripts/design_workbench.py
No image mockup or HTML renderer is used: every element is a native Craft object.
"""
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'design'
CLI = Path('/Applications/DesignCraft.app/Contents/MacOS/designcraft-cli')
C = {
    'bg':'#F5F5F5', 'rail':'#FFFFFF', 'panel':'#FFFFFF', 'panelAlt':'#F5F5F5',
    'border':'#E6E6E6', 'borderStrong':'#D9D9D9', 'ink':'#1E1E1E', 'muted':'#6E6E6E', 'faint':'#8A8A8A',
    'teal':'#00875F', 'green':'#02DA96', 'onGreen':'#07352A', 'lavender':'#9747FF', 'amber':'#8D681B',
    'tealDim':'#E0FBF1', 'purpleDim':'#F2E8FF', 'dot':'#DEDEDE', 'white':'#FFFFFF',
    'tabbar':'#E8E8E8', 'brandTeal':'#6FE4CC', 'brandPurple':'#AAA4FF', 'brandBg':'#0B1020',
}
commands = []
def cmd(command_id, **params):
    commands.append({'command':command_id, 'params':params})
def rect(x,y,w,h,fill='panel',stroke=None,r=0,weight=1):
    cmd('frame.create',rect=[x,y,x+w,y+h],content='unassigned')
    cmd('object.fill',swatch=fill)
    cmd('object.stroke',swatch=stroke or '[None]',weight=weight if stroke else 0)
    if r: cmd('object.cornerOptions',shape='rounded',size=r)
def txt(x,y,w,h,text,size=14,color='ink',bold=False,align='left',family='Apple SD Gothic Neo'):
    cmd('frame.create',rect=[x,y,x+w,y+h],content='text',text=text,caret=False)
    cmd('object.fill',swatch='[None]')
    cmd('object.stroke',swatch='[None]',weight=0)
    cmd('object.textFrameOptions',inset=0)
    cmd('type.char',attrs={'fontFamily':family,'fontStyle':'Bold' if bold else 'Regular','size':size,'leading':{'kind':'points','value':size*1.36},'fill':color})
    cmd('type.para',attrs={'align':align,'hyphenate':False})
def line(points,color='border',width=1):
    cmd('path.create',anchors=[{'p':[x,y]} for x,y in points],closed=False)
    cmd('object.fill',swatch='[None]')
    cmd('object.stroke',swatch=color,weight=width,cap='round',join='round')
def circle(x,y,d,color='teal'):
    cmd('frame.create',rect=[x,y,x+d,y+d],shape='ellipse',content='unassigned')
    cmd('object.fill',swatch=color);cmd('object.stroke',swatch='[None]',weight=0)
def pill(x,y,w,label,color='teal',fill='tealDim',h=25):
    rect(x,y,w,h,fill,r=h/2)
    txt(x,y+4,w,h-3,label,11,color,True,'center')
def table_icon(x,y,color='muted',scale=1):
    line([(x,y),(x+15*scale,y),(x+15*scale,y+14*scale),(x,y+14*scale),(x,y)],color,1.3)
    line([(x,y+5*scale),(x+15*scale,y+5*scale)],color,1.1)
    line([(x+5*scale,y+5*scale),(x+5*scale,y+14*scale)],color,1.1)
def button(x,y,w,h,label,primary=False):
    rect(x,y,w,h,'green' if primary else 'panelAlt',None if primary else 'border',5)
    txt(x,y+(h-19)/2,w,22,label,13,'onGreen' if primary else 'ink',True,'center')
def field(x,y,w,key,label,kind,selected=False):
    if selected:rect(x-10,y-5,w+20,32,'tealDim',r=4)
    if key:pill(x,y,26,key,'amber' if key=='PK' else 'teal','panelAlt',21)
    txt(x+36,y+1,w-100,24,label,12,'ink',False,family='Arial')
    txt(x+w-65,y+2,65,22,kind,11,'muted',False,'right',family='Arial')
def card(x,y,w,h,name,source,fields,color='teal',selected=False):
    rect(x,y,w,h,'panel','green' if selected else 'borderStrong',5,1.5 if selected else 1)
    rect(x+1,y+1,w-2,52,'panel',r=4)
    line([(x+1,y+52),(x+w-1,y+52)],'border')
    table_icon(x+18,y+19,color)
    txt(x+44,y+15,w-64,28,name,15,'ink',True,family='Arial')
    for i,(key,label,kind) in enumerate(fields):field(x+18,y+69+i*34,w-36,key,label,kind,selected and key=='FK')
    line([(x+16,y+h-34),(x+w-16,y+h-34)],'border')
    txt(x+18,y+h-25,w-36,20,source,10,'muted')

cmd('file.new',title='StructureWeaver · FigCraft-style Native Workbench',intent='web',units='points',width=1440,height=960,pages=1,margins=0,facingPages=False)
for name,color in C.items():cmd('swatch.create',name=name,color=color)
rect(0,0,1440,960,'bg')

# FigCraft source reference: 40px tabbar, neutral light chrome, no dashboard banner.
rect(0,0,1440,40,'tabbar')
line([(28,14),(22,20),(22,28),(33,28),(33,20),(28,14)],'ink',1.2)
rect(66,0,264,40,'panel')
rect(79,9,22,22,'brandBg',r=5)
line([(85,14),(85,26),(96,26),(96,14)],'brandTeal',1.5)
line([(84,18),(97,18)],'brandPurple',1.5)
line([(89,14),(89,27)],'brandPurple',1.5)
line([(84,23),(97,23)],'brandTeal',1.5)
txt(112,13,180,20,'StructureWeaver · demo.xlsx',12,'ink',False,family='Arial')
txt(307,12,16,20,'×',15,'muted',False,'center')
txt(346,11,24,22,'+',18,'muted',False,'center')
circle(1390,12,13,'muted')
circle(1394,9,13,'tabbar')

# Compact white tool rail and source panel, matching FigCraft's files/layers layout.
rect(0,40,52,920,'rail')
line([(52,40),(52,960)],'border')
rect(52,40,220,920,'panel')
line([(272,40),(272,960)],'border')
for y,label,selected in [(82,'파일',False),(151,'구조',False),(220,'ERD',True),(289,'리포트',False)]:
    if selected:rect(9,y-1,34,30,'tealDim',r=5)
    table_icon(18,y+7,'teal' if selected else 'ink',.95)
    txt(2,y+37,48,20,label,10,'ink',False,'center')
txt(68,59,166,24,'StructureWeaver',14,'ink',True,family='Arial')
txt(68,86,170,20,'로컬 프로젝트',11,'muted')
button(68,119,188,29,'워크북 열기')
line([(52,164),(272,164)],'border')
txt(68,182,164,20,'시트',12,'ink',True)
txt(235,177,20,24,'+',18,'muted',False,'center')
rect(64,215,196,32,'panelAlt',r=5)
txt(75,224,176,20,'전체 시트',12,'ink',True)
line([(52,264),(272,264)],'border')
txt(68,282,166,22,'테이블',12,'ink',True)
for y,name,source,count,selected in [
    (321,'Customers','A1:C4','3개 필드',False),
    (409,'Orders','A1:C5','3개 필드',True),
    (497,'LineItems','A1:D6','4개 필드',False)]:
    if selected:rect(64,y-4,196,32,'tealDim',r=5)
    table_icon(76,y+5,'teal' if selected else 'muted',.8)
    txt(99,y+1,148,22,name,12,'ink',selected,family='Arial')
    txt(99,y+31,144,18,source+' · '+count,10,'muted')
    txt(99,y+54,144,18,'전체 범위 분석 완료',10,'faint')
line([(52,799),(272,799)],'border')
txt(68,820,182,22,'추론 엔진',11,'muted',True)
circle(69,856,6,'green')
txt(83,849,171,23,'Llama · 로컬 연결',12,'ink')
rect(68,886,188,28,'panelAlt',r=5)
txt(78,894,164,19,'Gemma · 이미지 분석',10,'muted')

# The canvas remains the primary surface; title and status are intentionally compact.
txt(304,70,470,22,'demo.xlsx  /  ERD 초안',12,'muted')
txt(304,108,190,30,'관계 지도',20,'ink',True)
for x,label in [(469,'시트 3'),(542,'테이블 3'),(627,'관계 후보 2')]:
    rect(x,107,65 if x==469 else 77 if x==542 else 91,28,'panel','border',5)
    txt(x,115,65 if x==469 else 77 if x==542 else 91,20,label,11,'muted',False,'center')
txt(304,151,756,25,'셀의 위치와 실제 값을 근거로 연결한 데이터 모델입니다.',12,'muted')
for xx in range(308,1138,28):
    for yy in range(206,863,28):rect(xx,yy,1,1,'dot')

# ERD edges refer to actual PK/FK rows, and selected attributes use FigCraft green.
line([(636,357),(708,357),(708,343),(790,343)],'teal',1.4)
circle(632,353,8,'teal');circle(786,339,8,'teal')
pill(682,366,52,'1 : N','teal','tealDim',23)
line([(790,310),(750,310),(750,662),(790,662)],'lavender',1.4)
circle(786,306,8,'lavender');circle(786,658,8,'lavender')
pill(724,492,52,'1 : N','lavender','purpleDim',23)
card(340,287,296,226,'Customers','Customers!A1:C4 · 3 records',[
    ('PK','customer_id','INTEGER'),('','customer_name','TEXT'),('','country','TEXT')],color='teal')
card(790,240,306,226,'Orders','Orders!A1:C5 · 4 records',[
    ('PK','order_id','INTEGER'),('FK','customer_id','INTEGER'),('','amount','DECIMAL')],color='teal',selected=True)
card(790,559,306,242,'LineItems','LineItems!A1:D6 · 5 records',[
    ('PK','line_id','INTEGER'),('FK','order_id','INTEGER'),('','product_name','TEXT'),('','quantity','INTEGER')],color='lavender')
# Small selection handles echo the source editor, while preserving app identity.
for x,y in [(787,237),(1093,237),(787,463),(1093,463)]:rect(x,y,6,6,'panel','green',0,1)
rect(340,619,296,81,'panel','border',5)
txt(356,635,264,23,'원본에 연결된 ERD',12,'ink',True)
txt(356,661,264,27,'테이블을 선택해 셀 범위와 근거를 확인하세요.',11,'muted')
circle(307,928,5,'green')
txt(320,921,276,22,'전체 3개 시트 분석 완료',11,'muted')
txt(879,921,252,22,'자동 추론 결과 · 검토 필요',11,'faint',False,'right')

# White property inspector with compact sections and source evidence.
rect(1162,40,278,920,'panel')
line([(1162,40),(1162,960)],'border')
circle(1180,57,24,'teal')
txt(1180,62,24,19,'S',11,'white',True,'center',family='Arial')
button(1263,52,157,32,'결과 내보내기',True)
line([(1162,99),(1440,99)],'border')
rect(1172,110,53,26,'panelAlt',r=5)
txt(1172,117,53,20,'관계',11,'ink',False,'center')
txt(1238,117,74,20,'원본 구조',11,'muted')
txt(1360,117,61,20,'100%',11,'muted',False,'right',family='Arial')
line([(1162,147),(1440,147)],'border')
txt(1180,167,218,23,'Orders → Customers',14,'ink',True,family='Arial')
txt(1180,196,235,21,'customer_id',12,'teal',False,family='Arial')
line([(1162,235),(1440,235)],'border')
txt(1180,254,223,22,'관계 속성',12,'ink',True)
for y,label,value in [(291,'유형','N : 1'),(329,'상태','검토할 후보'),(367,'일치한 값','4 / 4')]:
    txt(1180,y+6,78,20,label,11,'muted')
    rect(1265,y,155,28,'panelAlt',r=5)
    txt(1276,y+7,132,20,value,11,'ink')
line([(1162,416),(1440,416)],'border')
txt(1180,436,226,22,'추론 근거',12,'ink',True)
for y,title,desc in [
    (477,'값 포함 관계','주문의 고객 ID가 고객 키에 포함됩니다.'),
    (542,'키 유일성','고객 키에 빈 값과 중복이 없습니다.'),
    (607,'이름과 자료형','필드 이름과 자료형이 호환됩니다.')]:
    circle(1181,y+4,13,'tealDim')
    line([(1185,y+10),(1188,y+13),(1192,y+7)],'teal',1)
    txt(1203,y,216,23,title,11,'ink',True)
    txt(1180,y+28,239,30,desc,10,'muted')
line([(1162,669),(1440,669)],'border')
txt(1180,690,228,22,'원본 범위',12,'ink',True)
rect(1180,725,240,29,'panelAlt',r=5)
txt(1192,733,216,20,'Orders!B2:B5',11,'teal',False,family='Arial')
txt(1180,767,240,20,'↓',12,'faint',False,'center')
rect(1180,798,240,29,'panelAlt',r=5)
txt(1192,806,216,20,'Customers!A2:A4',11,'ink',False,family='Arial')
line([(1162,851),(1440,851)],'border')
txt(1180,875,240,50,'관계는 관측된 데이터의 후보입니다.\n업무 규칙을 확인한 뒤 확정하세요.',11,'muted')

# FigCraft's defining bottom floating toolbar: 48 high, 13 radius, active green.
rect(490,886,444,50,'border',r=14)
rect(492,884,440,48,'panel','borderStrong',13)
rect(500,892,32,32,'green',r=5)
line([(511,900),(511,916),(515,912),(519,919),(522,917),(518,910),(525,910),(511,900)],'onGreen',1.3)
line([(551,904),(556,909),(561,904)],'muted',1.2)
line([(582,895),(582,921)],'border')
table_icon(600,900,'ink',1.1)
line([(640,904),(650,914),(663,901)],'ink',1.4)
circle(636,900,5,'panel');circle(659,897,5,'panel')
line([(688,900),(688,917),(705,917),(705,900),(688,900)],'ink',1.2)
txt(723,898,25,25,'T',16,'ink',False,'center',family='Arial')
line([(764,895),(764,921)],'border')
rect(779,895,61,26,'panelAlt',r=5)
txt(779,902,61,19,'100%',11,'ink',False,'center',family='Arial')
button(852,894,68,28,'원본')

cmd('file.info',documentTitle='StructureWeaver Native Workbench',author='StructureWeaver',description='Editable StructureWeaver workbench generated with DesignCraft CLI, following FigCraft light chrome and tokens at commit 8eb0b87646c5aef712326d6c431ccde5408630de.')
cmd('document.inspect')
cmd('font.list')
cmd('preflight.run')
OUT.mkdir(exist_ok=True)
source = OUT/'structureweaver-workbench.commands.json'
source.write_text(json.dumps(commands,ensure_ascii=False,indent=2)+'\n')
result = subprocess.run([str(CLI),'script',str(source),'--save',str(OUT/'structureweaver-workbench.designcraft'),'--export',str(OUT/'structureweaver-workbench.png')],capture_output=True,text=True,check=True)
report = json.loads(result.stdout)
if report.get('error'):
    raise RuntimeError(json.dumps(report,ensure_ascii=False))
results = report['results']
inspect = results[-3]
fonts = results[-2]
preflight = results[-1]
validation = {
    'tool':subprocess.check_output([str(CLI),'--version'],text=True).strip(),
    'commands_completed':report.get('completed',len(results)),
    'editable_items':sum(len(s.get('items',[])) for s in inspect.get('spreads',[])),
    'text_stories':len(inspect.get('stories',[])),
    'overset_stories':[s for s in inspect.get('stories',[]) if s.get('overset')],
    'fonts':fonts,
    'preflight':preflight,
    'document_size':[1440,960],
    'style_reference':{'repository':'https://github.com/storytold/figcraft','commit':'8eb0b87646c5aef712326d6c431ccde5408630de','theme':'light'},
}
(OUT/'structureweaver-workbench.validation.json').write_text(json.dumps(validation,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({k:v for k,v in validation.items() if k not in ('fonts','preflight')},ensure_ascii=False))
print('Saved native .designcraft, PNG, commands JSON and validation JSON in design/.')
