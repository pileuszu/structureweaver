#!/usr/bin/env python3
"""Synthetic 3D/effect/layout regression workbook (development only)."""
from copy import deepcopy
from datetime import datetime
from io import BytesIO
from pathlib import Path
from zipfile import ZipFile, ZipInfo, ZIP_DEFLATED
import xml.etree.ElementTree as ET

import openpyxl
from openpyxl.chart import (
    AreaChart3D, BarChart, BarChart3D, LineChart3D, PieChart3D,
    SurfaceChart3D, Reference,
)
from openpyxl.chart._3d import View3D
from openpyxl.chart.layout import Layout, ManualLayout
from openpyxl.chart.label import DataLabelList
from openpyxl.styles import Font, PatternFill, Alignment

ROOT = Path(__file__).resolve().parents[1]
wb = openpyxl.Workbook()
wb._named_styles['Normal'].font = Font(name='Arial', size=11)
wb.properties.creator = 'StructureWeaver synthetic fixture'
wb.properties.created = wb.properties.modified = datetime(2026, 11, 8)
data = wb.active
data.title = 'Data'
for row in [
    ('Quarter', 'Northern region — actual', 'Southern region — actual', 'Forecast'),
    ('Quarter 1', 18, 10, 12), ('Quarter 2', 32, 22, 25),
    ('Quarter 3', -6, 16, 30), ('Quarter 4', 28, 35, 38),
]:
    data.append(row)
for cell in data[1]:
    cell.font = Font(name='Arial', bold=True)

def sheet(name, title):
    ws = wb.create_sheet(name)
    ws.sheet_view.showGridLines = False
    ws.sheet_view.zoomScale = 100
    ws.sheet_format.defaultRowHeight = 20
    ws.sheet_format.customHeight = True
    for col in 'ABCDEFGHIJKLMNOPQRST':
        ws.column_dimensions[col].width = 12
    ws.merge_cells('A1:P2')
    ws['A1'] = title
    ws['A1'].font = Font(name='Arial', size=18, color='FFFFFF', bold=True)
    ws['A1'].fill = PatternFill('solid', fgColor='18344B')
    ws['A1'].alignment = Alignment(vertical='center')
    return ws

ws = sheet('3D charts', '3D CHARTS · SAVED CAMERA AND DEPTH')
for title, kind, anchor, end_col in [
    ('3D columns', BarChart3D, 'A4', 3),
    ('3D pie', PieChart3D, 'H4', 2),
    ('3D line', LineChart3D, 'A23', 3),
    ('3D area', AreaChart3D, 'H23', 3),
    ('3D cylinders', BarChart3D, 'A42', 3),
    ('3D surface', SurfaceChart3D, 'H42', 4),
]:
    chart = kind()
    chart.title = title
    chart.width, chart.height = 15, 11
    chart.add_data(Reference(data, min_col=2, max_col=end_col, min_row=1, max_row=5), titles_from_data=True)
    chart.set_categories(Reference(data, min_col=1, min_row=2, max_row=5))
    chart.view3D = View3D(rotX=25, rotY=30, depthPercent=120, rAngAx=False, perspective=25)
    if kind is PieChart3D:
        chart.view3D.rotX = 35
        chart.dataLabels = DataLabelList()
        chart.dataLabels.showPercent = True
    if title == '3D cylinders':
        chart.shape = 'cylinder'
    ws.add_chart(chart, anchor)

layout = sheet('Layout', 'CHART LAYOUT · AUTOMATIC AND SAVED POSITIONS')
for is_manual, anchor in [(False, 'A4'), (True, 'A25')]:
    chart = BarChart()
    chart.title = ('Manual: saved title, legend and plot coordinates' if is_manual else
                   'Automatic layout with a long title and multiple long series names')
    chart.add_data(Reference(data, min_col=2, max_col=4, min_row=1, max_row=5), titles_from_data=True)
    chart.set_categories(Reference(data, min_col=1, min_row=2, max_row=5))
    chart.width, chart.height = 23, 13
    chart.x_axis.title, chart.y_axis.title = 'Reporting quarter', 'Revenue (millions)'
    chart.legend.position = 'b'
    if is_manual:
        chart.layout = Layout(manualLayout=ManualLayout(layoutTarget='inner', xMode='edge', yMode='edge', x=0.14, y=0.22, w=0.66, h=0.58))
        chart.title.layout = Layout(manualLayout=ManualLayout(xMode='edge', yMode='edge', x=0.12, y=0.03, w=0.75, h=0.12))
        chart.legend.layout = Layout(manualLayout=ManualLayout(xMode='edge', yMode='edge', x=0.23, y=0.86, w=0.65, h=0.10))
    layout.add_chart(chart, anchor)

effects = sheet('Effects', 'DRAWING EFFECTS · SHADOW · GLOW · REFLECTION · 3D')
# Seed a drawing relationship; add custom DrawingML objects to this part below.
seed = BarChart()
seed.title = 'Effects stay anchored while scrolling'
seed.add_data(Reference(data, min_col=2, min_row=1, max_row=5), titles_from_data=True)
seed.width, seed.height = 15, 8
effects.add_chart(seed, 'A30')
wb.active = wb.index(ws)
buffer = BytesIO()
wb.save(buffer)
with ZipFile(BytesIO(buffer.getvalue())) as z:
    parts = {name: z.read(name) for name in z.namelist()}

# Some openpyxl 3D chart classes omit the view3D property when serializing.
# Persist the explicit camera for all six chart families in OOXML itself.
C = 'http://schemas.openxmlformats.org/drawingml/2006/chart'
ET.register_namespace('c', C)
for index in range(1, 7):
    part = f'xl/charts/chart{index}.xml'
    chart_root = ET.fromstring(parts[part])
    chart = chart_root.find(f'{{{C}}}chart')
    old = chart.find(f'{{{C}}}view3D')
    if old is not None:
        chart.remove(old)
    view = ET.fromstring(f'<c:view3D xmlns:c="{C}"><c:rotX val="{35 if index == 2 else 25}"/><c:rotY val="30"/><c:depthPercent val="120"/><c:rAngAx val="0"/><c:perspective val="25"/></c:view3D>')
    chart.insert(list(chart).index(chart.find(f'{{{C}}}plotArea')), view)
    parts[part] = ET.tostring(chart_root, encoding='utf-8', xml_declaration=True)

XDR = 'http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing'
A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
ET.register_namespace('xdr', XDR)
ET.register_namespace('a', A)
root = ET.fromstring(parts['xl/drawings/drawing3.xml'])
def shape(idx, label, col, row, extra):
    return ET.fromstring(f'''<xdr:oneCellAnchor xmlns:xdr="{XDR}" xmlns:a="{A}">
    <xdr:from><xdr:col>{col}</xdr:col><xdr:colOff>152400</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>152400</xdr:rowOff></xdr:from>
    <xdr:ext cx="2286000" cy="914400"/>
    <xdr:sp><xdr:nvSpPr><xdr:cNvPr id="{idx}" name="{label}"/><xdr:cNvSpPr/></xdr:nvSpPr>
    <xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="2286000" cy="914400"/></a:xfrm>
    <a:prstGeom prst="roundRect"><a:avLst/></a:prstGeom>
    <a:solidFill><a:srgbClr val="16A085"/></a:solidFill>
    <a:ln w="19050"><a:solidFill><a:srgbClr val="087F6B"/></a:solidFill></a:ln>{extra}</xdr:spPr>
    <xdr:txBody><a:bodyPr anchor="ctr"/><a:lstStyle/><a:p><a:pPr algn="ctr"/>
    <a:r><a:rPr sz="1400" b="1"><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill><a:latin typeface="Arial"/></a:rPr><a:t>{label}</a:t></a:r>
    <a:endParaRPr/></a:p></xdr:txBody></xdr:sp><xdr:clientData/></xdr:oneCellAnchor>''')

examples = [
    ('Outer shadow', 0, 4, '<a:effectLst><a:outerShdw blurRad="76200" dist="76200" dir="2700000" algn="ctr" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="45000"/></a:srgbClr></a:outerShdw></a:effectLst>'),
    ('Inner shadow', 4, 4, '<a:effectLst><a:innerShdw blurRad="50800" dist="38100" dir="2700000"><a:srgbClr val="00382D"><a:alpha val="70000"/></a:srgbClr></a:innerShdw></a:effectLst>'),
    ('Glow', 8, 4, '<a:effectLst><a:glow rad="127000"><a:srgbClr val="F59E0B"><a:alpha val="80000"/></a:srgbClr></a:glow></a:effectLst>'),
    ('Soft edges', 0, 12, '<a:effectLst><a:softEdge rad="76200"/></a:effectLst>'),
    ('Reflection', 4, 12, '<a:effectLst><a:reflection blurRad="12700" stA="55000" stPos="0" endA="0" endPos="100000" dist="38100" dir="5400000" fadeDir="5400000" sx="100000" sy="-100000" algn="bl" rotWithShape="0"/></a:effectLst>'),
    ('Bevel', 8, 12, '<a:scene3d><a:camera prst="orthographicFront"/><a:lightRig rig="threePt" dir="t"/></a:scene3d><a:sp3d prstMaterial="matte"><a:bevelT w="76200" h="76200" prst="circle"/></a:sp3d>'),
    ('Extrusion', 0, 20, '<a:scene3d><a:camera prst="isometricRightUp"/><a:lightRig rig="threePt" dir="t"/></a:scene3d><a:sp3d extrusionH="254000" prstMaterial="matte"><a:bevelT w="38100" h="38100" prst="circle"/><a:extrusionClr><a:srgbClr val="087F6B"/></a:extrusionClr></a:sp3d>'),
    ('Combined effects', 4, 20, '<a:effectLst><a:glow rad="50800"><a:srgbClr val="A7F3D0"/></a:glow><a:outerShdw blurRad="76200" dist="101600" dir="2700000"><a:srgbClr val="000000"><a:alpha val="40000"/></a:srgbClr></a:outerShdw></a:effectLst>'),
]
for idx, (label, col, row, extra) in enumerate(examples, 100):
    root.append(shape(idx, label, col, row, extra))
parts['xl/drawings/drawing3.xml'] = ET.tostring(root, encoding='utf-8', xml_declaration=True)
out = ROOT / 'fixtures/fidelity.xlsx'
with ZipFile(out, 'w', ZIP_DEFLATED) as z:
    for name, value in sorted(parts.items()):
        info = ZipInfo(name, (2026, 11, 8, 0, 0, 0))
        info.compress_type = ZIP_DEFLATED
        z.writestr(info, value)
print(out)
