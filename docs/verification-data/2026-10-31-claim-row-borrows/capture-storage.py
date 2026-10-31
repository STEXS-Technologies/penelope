from pathlib import Path
import json,os,subprocess,tempfile,sys
p=tempfile.gettempdir()
data={'TMPDIR':os.environ.get('TMPDIR'),'tempdir':p,'filesystem':subprocess.check_output(['findmnt','-T',p,'-o','TARGET,SOURCE,FSTYPE,OPTIONS','-n'],text=True)}
Path(sys.argv[1]).write_text(json.dumps(data,indent=2)+'\n')
