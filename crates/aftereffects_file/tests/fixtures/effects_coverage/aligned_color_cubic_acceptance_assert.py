import hashlib,json,math,pathlib
root=pathlib.Path(__file__).resolve().parent
artifact=json.loads((root/'aligned_color_cubic_acceptance.json').read_text())
r=artifact['native_readback']
assert hashlib.sha256((root/'aligned_color_cubic_native.aep').read_bytes()).hexdigest()==artifact['summary']['independent_oracle_sha256']
for exported in artifact['exports']:
 assert hashlib.sha256((root/f"aligned_color_cubic_{exported['mode']}.fx.json").read_bytes()).hexdigest()==exported['source_fx_sha256']
def curve(x):
 lo,hi=0.,1.
 for _ in range(80):
  u=(lo+hi)/2;v=3*(1-u)**2*u*.25+3*(1-u)*u*u*.75+u**3
  if v<x:lo=u
  else:hi=u
 u=(lo+hi)/2
 return 3*(1-u)**2*u*.1+3*(1-u)*u*u*.9+u**3
worst=0
for name,end in [('before',[.8,.7,.6]),('after',[.9,.7,.6]),('reference',[.9,.7,.6])]:
 phase=r[name]; assert phase['effectEnabled'] is True
 assert [k['time'] for k in phase['keys']]==[0,1]
 assert phase['keys'][0]['outInterpolation']=='6613' and phase['keys'][1]['inInterpolation']=='6613'
 distance=255*math.dist([.1,.2,.3],end)
 assert len(phase['keys'][0]['outgoing'])==len(phase['keys'][1]['incoming'])==1
 for ease in [phase['keys'][0]['outgoing'][0],phase['keys'][1]['incoming'][0]]:
  assert abs(ease['speed']-.4*distance)<1e-7 and ease['influence']==25,(name,ease,distance)
 for sample in phase['samples']:
  for c,base in enumerate([.1,.2,.3]):
   error=abs(sample['value'][c]-(base+(end[c]-base)*curve(sample['time'])))
   worst=max(worst,error);assert error<5e-7,(name,sample,c,error)
  assert sample['value'][3]==0
 assert phase['amount']==73
 assert max(abs(x-y) for x,y in zip(phase['white'],[.52,1,.7,0]))<1e-7
for before,after,reference in zip(r['before']['samples'],r['after']['samples'],r['reference']['samples']):
 assert max(abs(before['value'][c]-after['value'][c]) for c in [1,2,3])<5e-7
 assert max(abs(a-b) for a,b in zip(after['value'],reference['value']))<5e-7
 assert abs(after['value'][0]-before['value'][0]-.1*curve(after['time']))<5e-7
summary={'native_job':'1c9d606a100a41498c1371602a06b6a8','samples':21,'worst_absolute_channel_error':worst,'tolerance':5e-7,'input_edit':'source FX red endpoint .8 to .9, fresh exports; green/blue/alpha/white/amount unchanged','independent_oracle_sha256':'cd84c6fc05ab2c5f960c0660fcc367d18ad013089cc987596e29c185c60e1c2e','proof':'fresh generated AEP native keys/ease/control curve acceptance; no render/alpha/kernel/Asset proof'}
assert summary == artifact['summary']
print(json.dumps(summary,indent=2))
