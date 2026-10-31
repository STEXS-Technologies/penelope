from pathlib import Path
import gzip,json,math,random,statistics,sys
out=Path(sys.argv[1]) if len(sys.argv)>1 else Path(__file__).resolve().parent
raw=out/'raw.jsonl';rows=[json.loads(line) for line in (raw.read_text() if raw.exists() else gzip.decompress((out/'raw.jsonl.gz').read_bytes()).decode()).splitlines()]
assert len(rows)==96
identities=set();checksums=set();latency=[]
for row in rows:
 identity=tuple(row[k] for k in ['campaign','block','slot','round']);assert identity not in identities;identities.add(identity)
 c,b,s,r=identity;assert c in range(2) and b in range(6) and s in range(4) and r in range(2)
 order=['one','two','two','one'] if b%2==0 else ['two','one','one','two'];assert row['variant']==order[s]
 assert row['batch']==128 and row['width']==128 and row['work']==0 and row['reopen'] is False
 assert row['schema']==1 and row['verified'] and row['consumed']==128 and row['outcomes']==129 and row['direct_first']==(r==0)
 assert row['reopen_ns']==0 and row['managed_ns']==sum(row[k] for k in ['schedule_ns','delivery_ns','reopen_ns','consume_ns'])
 checksums.add(row['checksum'])
assert len(checksums)==1
for campaign in range(2):
 for metric in ['schedule_ns','delivery_ns','consume_ns','managed_ns']:
  logs=[]
  for block in range(6):
   groups={variant:[r[metric] for r in rows if r['campaign']==campaign and r['block']==block and r['variant']==variant] for variant in ['one','two']}
   assert all(len(g)==4 for g in groups.values())
   logs.append(statistics.mean(math.log(n) for n in groups['two'])-statistics.mean(math.log(n) for n in groups['one']))
  rng=random.Random(20261031);boot=sorted(statistics.mean(rng.choices(logs,k=6)) for _ in range(20000))
  convert=lambda n:100*math.expm1(n)
  latency.append(dict(campaign=campaign,metric=metric,change_pct=convert(statistics.mean(logs)),ci_low=convert(boot[500]),ci_high=convert(boot[19499]),one_median_ns_per_timer=statistics.median(r[metric]/128 for r in rows if r['campaign']==campaign and r['variant']=='one'),two_median_ns_per_timer=statistics.median(r[metric]/128 for r in rows if r['campaign']==campaign and r['variant']=='two')))
summary=dict(rows=len(rows),timers=sum(row['consumed'] for row in rows),latency=latency)
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
for row in latency:
 if row['metric']=='managed_ns':print(row)
