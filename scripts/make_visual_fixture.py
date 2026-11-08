#!/usr/bin/env python3
"""Reproducible synthetic Excel visuals fixture; development-only openpyxl/Pillow."""
from pathlib import Path
from io import BytesIO
from copy import deepcopy
from zipfile import ZipFile, ZipInfo, ZIP_DEFLATED
import xml.etree.ElementTree as ET
from datetime import datetime
import openpyxl
from openpyxl.chart import BarChart, LineChart, PieChart, DoughnutChart, ScatterChart, AreaChart, Reference, Series
from openpyxl.chart.label import DataLabelList
from openpyxl.formatting.rule import CellIsRule, FormulaRule, ColorScaleRule, DataBarRule, IconSetRule, Rule
from openpyxl.styles import Font, PatternFill, Alignment
from openpyxl.styles.differential import DifferentialStyle
from openpyxl.drawing.image import Image
from PIL import Image as PillowImage

ROOT = Path(__file__).resolve().parents[1]
wb = openpyxl.Workbook()
wb._named_styles['Normal'].font = Font(name='Arial',size=11)
wb.properties.creator = 'StructureWeaver synthetic fixture'
wb.properties.created = wb.properties.modified = datetime(2026,11,8)
ws = wb.active
ws.title = 'Visuals'
ws.sheet_view.showGridLines = False
ws.sheet_view.zoomScale = 100
ws.sheet_format.defaultRowHeight = 20
ws.sheet_format.customHeight = True
for col in 'ABCDEFGHIJKLMNO':
    ws.column_dimensions[col].width = 12
ws.merge_cells('A1:L2')
ws['A1'] = 'CONDITIONAL FORMATS · CHARTS · DRAWINGS'
ws['A1'].font = Font(name='Arial',size=17,bold=True,color='FFFFFF')
ws['A1'].fill = PatternFill('solid',fgColor='18344B')
ws['A1'].alignment = Alignment(vertical='center')
headers = ['Value','Duplicate','Formula','Data bar','Scale','Icon']
for c, text in enumerate(headers,1):
    cell=ws.cell(4,c,text); cell.font=Font(name='Arial',bold=True,color='FFFFFF'); cell.fill=PatternFill('solid',fgColor='08917A')
values = [-40,-10,0,10,25,40,55,70,85,100]
for i,v in enumerate(values,5):
    for j,value in enumerate([v, ['A','B','A','C','D','C','E','F','F','G'][i-5], v,v,v,v],1):
        cell=ws.cell(i,j,value); cell.font=Font(name='Arial',size=11); cell.alignment=Alignment(vertical='center')
red=PatternFill('solid',fgColor='FECACA',bgColor='FECACA'); green=PatternFill('solid',fgColor='A7F3D0',bgColor='A7F3D0')
ws.conditional_formatting.add('A5:A14',CellIsRule(operator='lessThan',formula=['0'],fill=red,stopIfTrue=True))
ws.conditional_formatting.add('A5:A14',CellIsRule(operator='greaterThanOrEqual',formula=['70'],fill=green))
ws.conditional_formatting.add('B5:B14',Rule(type='duplicateValues',dxf=DifferentialStyle(fill=PatternFill('solid',fgColor='DDD6FE',bgColor='DDD6FE'))))
ws.conditional_formatting.add('C5:C14',FormulaRule(formula=['AND($A5>=0,MOD(ROW(),2)=0)'],fill=PatternFill('solid',fgColor='FEF3C7',bgColor='FEF3C7'),font=Font(color='92400E',bold=True)))
ws.conditional_formatting.add('D5:D14',DataBarRule(start_type='min',end_type='max',color='16A085',showValue=True))
ws.conditional_formatting.add('E5:E14',ColorScaleRule(start_type='min',start_color='F8696B',mid_type='percentile',mid_value=50,mid_color='FFEB84',end_type='max',end_color='63BE7B'))
ws.conditional_formatting.add('F5:F14',IconSetRule('3Arrows','percent',[0,33,67],showValue=True))
ws['A16']='Anchored vector objects and embedded image'; ws['A16'].font=Font(name='Arial',bold=True,color='334155')
ws['A26']='Scroll and zoom: objects retain worksheet coordinates.'; ws['A26'].font=Font(name='Arial',italic=True,size=10,color='64748B')

cs=wb.create_sheet('Chart data')
for row in [('Quarter','North','South','Growth'),('Q1',15,10,5),('Q2',24,14,8),('Q3',18,21,6),('Q4',35,28,13)]: cs.append(row)
for cell in cs[1]: cell.font=Font(bold=True)
bar=BarChart(); bar.title='Quarterly revenue'; bar.y_axis.title='Revenue'; bar.x_axis.title='Quarter'; bar.style=10
bar.add_data(Reference(cs,min_col=2,max_col=3,min_row=1,max_row=5),titles_from_data=True); bar.set_categories(Reference(cs,min_col=1,min_row=2,max_row=5))
bar.width=14; bar.height=9; ws.add_chart(bar,'H4')
pie=PieChart(); pie.title='North by quarter'; pie.add_data(Reference(cs,min_col=2,min_row=1,max_row=5),titles_from_data=True); pie.set_categories(Reference(cs,min_col=1,min_row=2,max_row=5)); pie.style=10; pie.width=12; pie.height=7
pie.dataLabels=DataLabelList(); pie.dataLabels.showPercent=True
ws.add_chart(pie,'H22')

charts=wb.create_sheet('Chart gallery'); charts.sheet_view.showGridLines=False
for col in 'ABCDEFGHIJKLMNOPQRST': charts.column_dimensions[col].width=12
charts.sheet_format.defaultRowHeight=20
charts.sheet_format.customHeight=True
for name,kind,anchor in [('Line',LineChart,'A1'),('Stacked columns',BarChart,'H1'),('Area',AreaChart,'A20'),('Doughnut',DoughnutChart,'H20')]:
    ch=kind(); ch.title=name; ch.width=13; ch.height=10
    ch.add_data(Reference(cs,min_col=2,max_col=3 if name!='Doughnut' else 2,min_row=1,max_row=5),titles_from_data=True)
    ch.set_categories(Reference(cs,min_col=1,min_row=2,max_row=5))
    if name=='Stacked columns': ch.grouping='stacked'; ch.overlap=100
    charts.add_chart(ch,anchor)
scatter=ScatterChart(); scatter.title='Scatter'; scatter.width=13; scatter.height=10
scatter.series.append(Series(Reference(cs,min_col=3,min_row=2,max_row=5),Reference(cs,min_col=2,min_row=2,max_row=5),title='South / North'))
charts.add_chart(scatter,'A39')
combo=BarChart(); combo.title='Combo'; combo.width=13; combo.height=10
combo.add_data(Reference(cs,min_col=2,max_col=3,min_row=1,max_row=5),titles_from_data=True); combo.set_categories(Reference(cs,min_col=1,min_row=2,max_row=5))
line=LineChart(); line.add_data(Reference(cs,min_col=4,min_row=1,max_row=5),titles_from_data=True); line.set_categories(Reference(cs,min_col=1,min_row=2,max_row=5)); combo += line
charts.add_chart(combo,'H39')
chart_sheet=wb.create_chartsheet('Chart sheet')
chart_sheet.add_chart(deepcopy(bar))

pic=PillowImage.new('RGB',(64,64)); pic.putdata([(int(x*4),int(y*4),140) for y in range(64) for x in range(64)])
png=BytesIO(); pic.save(png,format='PNG'); png.seek(0)
img=Image(png); img.width=64; img.height=64; ws.add_image(img,'F18')
buf=BytesIO(); wb.save(buf)
with ZipFile(BytesIO(buf.getvalue())) as z: parts={n:z.read(n) for n in z.namelist()}
XDR='http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing'; A='http://schemas.openxmlformats.org/drawingml/2006/main'
ET.register_namespace('xdr',XDR); ET.register_namespace('a',A)
root=ET.fromstring(parts['xl/drawings/drawing1.xml'])
def shape(id,name,preset,col,row,width,height,color,rotation=0,text=''):
    return ET.fromstring(f'''<xdr:oneCellAnchor xmlns:xdr="{XDR}" xmlns:a="{A}"><xdr:from><xdr:col>{col}</xdr:col><xdr:colOff>38100</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>38100</xdr:rowOff></xdr:from><xdr:ext cx="{width*12700}" cy="{height*12700}"/><xdr:sp><xdr:nvSpPr><xdr:cNvPr id="{id}" name="{name}"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr><a:xfrm rot="{rotation*60000}"><a:off x="0" y="0"/><a:ext cx="{width*12700}" cy="{height*12700}"/></a:xfrm><a:prstGeom prst="{preset}"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="{color}"/></a:solidFill><a:ln w="19050"><a:solidFill><a:srgbClr val="334155"/></a:solidFill></a:ln></xdr:spPr><xdr:txBody><a:bodyPr anchor="ctr"/><a:lstStyle/><a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="en-US" sz="1200" b="1"><a:solidFill><a:srgbClr val="18344B"/></a:solidFill><a:latin typeface="Arial"/></a:rPr><a:t>{text}</a:t></a:r><a:endParaRPr lang="en-US"/></a:p></xdr:txBody></xdr:sp><xdr:clientData/></xdr:oneCellAnchor>''')
source=shape(101,'Rounded rectangle','roundRect',0,17,130,55,'A7F3D0',text='Source')
sp=source.find(f'.//{{{XDR}}}spPr')
sp.remove(sp.find(f'{{{A}}}solidFill'))
sp.insert(2,ET.fromstring(f'<a:gradFill xmlns:a="{A}" rotWithShape="1"><a:gsLst><a:gs pos="0"><a:srgbClr val="A7F3D0"/></a:gs><a:gs pos="100000"><a:srgbClr val="FFFFFF"/></a:gs></a:gsLst><a:lin ang="0" scaled="1"/></a:gradFill>'))
root.append(source)
root.append(shape(102,'Arrow','rightArrow',2,18,75,32,'C4B5FD',text=''))
root.append(shape(103,'Decision','diamond',3,17,95,65,'FDE68A',text='Model'))
root.append(shape(104,'Rotated ellipse','ellipse',0,22,120,40,'BAE6FD',rotation=-12,text='Rotated'))
root.append(shape(105,'Triangle','triangle',2,22,60,45,'FCA5A5',text=''))
parts['xl/drawings/drawing1.xml']=ET.tostring(root,encoding='utf-8',xml_declaration=True)
# Reproducible package timestamps, no machine paths or user data.
out=ROOT/'fixtures/visuals.xlsx'
with ZipFile(out,'w',ZIP_DEFLATED) as z:
    for name,data in sorted(parts.items()):
        info=ZipInfo(name,(2026,11,8,0,0,0)); info.compress_type=ZIP_DEFLATED; z.writestr(info,data)
print(out)
