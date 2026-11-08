#!/usr/bin/env python3
"""Create/edit/export StructureWeaver's vector app icon in FigCraft's local engine.
Start FigCraft --control 47917 --automation-read-root "$PWD/design" --automation-write-root "$PWD/design".
"""
import argparse,json,socket
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--port',type=int,default=47917);args=p.parse_args()
root=Path(__file__).resolve().parent.parent/'design';root.mkdir(exist_ok=True)
steps=[]
def add(command,params,name=None):
    step={'command':command,'params':params}
    if name:step['as']=name
    steps.append(step)
add('frame.create',{'name':'StructureWeaver icon','x':0,'y':0,'width':512,'height':512,'props':{'fill':'#0B1020','cornerRadius':108,'clip':True}},'icon')
def rect(name,x,y,w,h,color,r=12):
    add('node.create',{'type':'rectangle','name':name,'parent':{'$ref':'icon'},'x':x,'y':y,'width':w,'height':h,'props':{'fill':color,'cornerRadius':r}})
rect('Vertical source A',152,100,48,312,'#6FE4CC',18)
rect('Vertical source B',264,100,48,312,'#6FE4CC',18)
rect('Header relation A',100,164,312,48,'#AAA4FF',18)
rect('Header relation B',100,284,312,48,'#AAA4FF',18)
rect('Underpass A',144,156,64,64,'#0B1020',0)
rect('Woven overpass A',152,148,48,80,'#6FE4CC',0)
rect('Underpass B',256,276,64,64,'#0B1020',0)
rect('Woven overpass B',264,268,48,80,'#6FE4CC',0)
request={'command':'command.batch','params':{'label':'StructureWeaver woven structure mark','commands':steps}}
(root/'brand.figcraft-commands.json').write_text(json.dumps(request,ensure_ascii=False,indent=2))
with socket.create_connection(('127.0.0.1',args.port),10) as s:
    s.settimeout(30);f=s.makefile('rwb');seq=0
    def call(method,params):
        global seq
        seq+=1;f.write((json.dumps({'id':seq,'method':method,'params':params})+'\n').encode());f.flush();reply=json.loads(f.readline())
        if not reply.get('ok'):raise RuntimeError(reply)
        return reply['result']
    call('engine.execute',{'command':'file.new','params':{'name':'StructureWeaver brand'}})
    result=call('engine.execute',request)
    # Use inspection instead of assuming generated ids.
    doc=call('document.inspect',{'depth':1})
    (root/'brand.inspection.json').write_text(json.dumps(doc,ensure_ascii=False,indent=2))
    call('engine.execute',{'command':'file.save','params':{'path':str(root/'brand.figcraft')}})
    # Select the named frame explicitly from the inspect tree.
    def frames(obj):
        if isinstance(obj,dict):
            if obj.get('name')=='StructureWeaver icon' and 'id' in obj:yield obj['id']
            for v in obj.values():yield from frames(v)
        elif isinstance(obj,list):
            for v in obj:yield from frames(v)
    ids=list(frames(doc))
    if not ids:raise RuntimeError('Icon frame missing after creation')
    for fmt,name,scale in [('png','brand.png',1),('svg','brand.svg',1),('png','brand@2x.png',2)]:
        out=call('engine.execute',{'command':'file.export','params':{'ids':ids,'format':fmt,'path':str(root/name),'scale':scale}})
        print(fmt, {k:v for k,v in out.items() if k!='data'})
