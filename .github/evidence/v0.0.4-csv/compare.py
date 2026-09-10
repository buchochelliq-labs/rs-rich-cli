import argparse,hashlib,json,os,subprocess,tempfile
from pathlib import Path
parser=argparse.ArgumentParser(description='Compare CSV behavior and exports against a pre-change binary')
parser.add_argument('--baseline',required=True,type=Path)
parser.add_argument('--binary',required=True,type=Path)
parser.add_argument('--output',required=True,type=Path)
args=parser.parse_args()
baseline=args.baseline.resolve()
current=args.binary.resolve()
env=dict(os.environ,TERM='xterm-256color',COLUMNS='100',LINES='36');env.pop('NO_COLOR',None)
fixtures={
 'mixed.csv':'name,count,note\nAlice,12,"two\nlines"\n田中,3,🙂\nshort\nextra,4,last,overflow\n\n',
 'numeric.csv':'1,2,3\n4,5,6\n7,8,9\n',
 'quoted.csv':'name;amount;note\nAlice;1.5;"say ""hello"""\nBob;;plain\n',
 'tabs.tsv':'name\tcount\tnote\nAlice\t12\tfirst\nBob\t2\tsecond\n',
 'empty.csv':'',
}
variants=[[],['--panel','rounded'],['--padding','1'],['--center'],['--right'],['--pager'],['--title','[bold]Data[/]','--caption','Values'],['--panel','rounded','--style','italic']]
results=[]
with tempfile.TemporaryDirectory() as tmp:
 root=Path(tmp)
 for name,data in fixtures.items():
  path=root/name;path.write_text(data)
  for width in [4,20,80]:
   for flags in variants:
    outputs=[]
    for binary in [baseline,current]:
     html=root/'out.html';svg=root/'out.svg'
     cmd=[str(binary),'--csv',str(path),'--width',str(width),*flags,'--export-html',str(html),'--export-svg',str(svg)]
     r=subprocess.run(cmd,capture_output=True,env=env,timeout=10)
     outputs.append((r.returncode,r.stdout,r.stderr,html.read_bytes() if html.exists() else b'',svg.read_bytes() if svg.exists() else b''))
     for out in [html,svg]:
      out.unlink(missing_ok=True)
    assert outputs[0]==outputs[1],(name,width,flags)
    results.append({'fixture':name,'width':width,'flags':flags,'exit_code':outputs[1][0],'stdout_sha256':hashlib.sha256(outputs[1][1]).hexdigest(),'html_sha256':hashlib.sha256(outputs[1][3]).hexdigest(),'svg_sha256':hashlib.sha256(outputs[1][4]).hexdigest()})
args.output.write_text(json.dumps(results,indent=2)+'\n')
print(f'{len(results)} combinations match baseline stdout/stderr/status/HTML/SVG exactly')
