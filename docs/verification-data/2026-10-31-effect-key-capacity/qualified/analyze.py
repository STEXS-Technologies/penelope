from pathlib import Path
import collections,gzip,json,math,random,statistics,sys
out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
def read(name):
 p=out/name
 return p.read_text() if p.exists() else gzip.decompress((out/(name+'.gz')).read_bytes()).decode()
lengths={'short':124,'inline-limit':189,'heap-first':194,'maximum':719,'mixed':321}
rows=[json.loads(line) for line in read('raw.jsonl').splitlines()];assert len(rows)==960
checksums={};keys=set();groups=collections.defaultdict(list)
for row in rows:
 assert row['profile'] in lengths and row['kind'] in range(4) and row['campaign'] in range(2) and row['block'] in range(6) and row['slot'] in range(4)
 order=['baseline','candidate','candidate','baseline'] if row['block']%2==0 else ['candidate','baseline','baseline','candidate']
 assert row['variant']==order[row['slot']] and row['iterations']==100000 and row['elapsed_ns']>0
 fixture=(row['profile'],row['kind']);assert row['encoded_len']==lengths[row['profile']]
 assert checksums.setdefault(fixture,row['checksum'])==row['checksum']
 key=tuple(row[k] for k in ['campaign','block','slot','profile','kind']);assert key not in keys;keys.add(key)
 groups[(row['campaign'],*fixture)].append(row)
latency=[]
for key,data in sorted(groups.items()):
 blocks=[]
 for block in range(6):
  values={label:[r['elapsed_ns']/r['iterations'] for r in data if r['block']==block and r['variant']==label] for label in ['baseline','candidate']}
  assert all(len(values[label])==2 for label in values)
  blocks.append(statistics.mean(map(math.log,values['candidate']))-statistics.mean(map(math.log,values['baseline'])))
 rng=random.Random(20261031);boot=sorted(math.expm1(statistics.mean(rng.choices(blocks,k=6)))*100 for _ in range(20000))
 latency.append(dict(zip(['campaign','profile','kind'],key))|{'baseline_ns':statistics.median(r['elapsed_ns']/r['iterations'] for r in data if r['variant']=='baseline'),'candidate_ns':statistics.median(r['elapsed_ns']/r['iterations'] for r in data if r['variant']=='candidate'),'change_pct':math.expm1(statistics.mean(blocks))*100,'ci_low':boot[500],'ci_high':boot[19499]})
assert len(latency)==40
alloc=[json.loads(line) for line in read('allocations.jsonl').splitlines()];assert len(alloc)==80
allocation=[];keys=set()
for row in alloc:
 key=tuple(row[k] for k in ['repeat','variant','profile','kind']);assert key not in keys;keys.add(key)
 assert row['repeat'] in [0,1] and row['variant'] in ['baseline','candidate']
for fixture,checksum in sorted(checksums.items()):
 result=dict(zip(['profile','kind'],fixture))
 for label in ['baseline','candidate']:
  selected=[r for r in alloc if (r['profile'],r['kind'])==fixture and r['variant']==label]
  assert len(selected)==2 and all(r['checksum']==checksum and r['encoded_len']==lengths[fixture[0]] for r in selected)
  values={tuple(r[k] for k in ['allocations','reallocations','requested_bytes']) for r in selected};assert len(values)==1
  result[label]=dict(zip(['allocations','reallocations','requested_bytes'],values.pop()))
  assert result[label]['allocations']==1
  if label=='candidate':assert result[label]=={'allocations':1,'reallocations':0,'requested_bytes':lengths[fixture[0]]}
  else:assert result[label]['reallocations']>0 and result[label]['requested_bytes']>lengths[fixture[0]]
 allocation.append(result)
public=[json.loads(line) for line in read('public-allocations.jsonl').splitlines()];assert len(public)==640
cases=['outbox_ack','outbox_renew','timer_ack','timer_schedule','timer_cancel'];keys=set();public_allocation=[]
for row in public:
 assert row['case'] in cases and row['sample'] in range(32) and row['repeat'] in [0,1] and row['variant'] in ['baseline','candidate']
 key=tuple(row[k] for k in ['repeat','variant','case','sample']);assert key not in keys;keys.add(key)
for case in cases:
 result={'case':case}
 for label in ['baseline','candidate']:
  selected=[r for r in public if r['case']==case and r['variant']==label];assert len(selected)==64
  result[label]={name:{'min':min(r[name] for r in selected),'median':statistics.median(r[name] for r in selected),'max':max(r[name] for r in selected)} for name in ['allocations','reallocations','requested_bytes']}
 for name in ['allocations','reallocations','requested_bytes']:
  if case.startswith('outbox') and name!='allocations':assert result['candidate'][name]['max']<result['baseline'][name]['min']
  else:assert result['candidate'][name]==result['baseline'][name]
 public_allocation.append(result)
assert sum(r['iterations'] for r in rows)==96000000
measurement=json.loads(read('measurement.json'));assert (measurement['ordinary_processes'],measurement['key_allocation_processes'],measurement['public_allocation_processes'])==(48,4,4)
(out/'summary.json').write_text(json.dumps({'latency':latency,'allocation':allocation,'public_allocation':public_allocation,'timed_encodings':96000000},indent=2)+'\n')
for row in latency:print(row)
for row in allocation:print(row)
for row in public_allocation:print(row)
