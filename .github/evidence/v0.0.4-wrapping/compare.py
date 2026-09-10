import argparse,hashlib,json,os,subprocess,tempfile
from pathlib import Path
parser=argparse.ArgumentParser(description='Compare Text/Markdown wrapping behavior and exports against a pre-change binary')
parser.add_argument('--baseline',required=True,type=Path)
parser.add_argument('--binary',required=True,type=Path)
parser.add_argument('--output',required=True,type=Path)
args=parser.parse_args()
baseline=args.baseline.resolve()
current=args.binary.resolve()
env=dict(os.environ,TERM='xterm-256color',COLUMNS='100',LINES='36');env.pop('NO_COLOR',None)
fixtures={
 'ascii': 'abcdefghijklmnopqrstuvwxyz' * 20,
 'unicode': '界é🙂❤️xyz' * 40 + '\n' + 'éclair 界 hello ' * 20,
 'whitespace': '\n\talpha\tbeta\n\n' + 'hello   world  ' * 20 + '\n   ',
 'zero-width': 'aaaaaaaa ⁠ ️ tail ' * 20,
 'empty': '',
 'markdown.md': '# Wrapping\n\n' + '**bold** *italic* 界é🙂❤️ hello ' * 20,
}
variants=[[],['--panel','rounded'],['--padding','1'],['--center'],['--right'],['--pager'],['--title','[bold]Data[/]','--caption','Values'],['--panel','rounded','--style','italic']]
results=[]
with tempfile.TemporaryDirectory() as tmp:
 root=Path(tmp)
 for name,data in fixtures.items():
  path=root/name;path.write_text(data)
  for width in [1,7,20,80]:
   for flags in variants:
    outputs=[]
    for binary in [baseline,current]:
     html=root/'out.html';svg=root/'out.svg'
     cmd=[str(binary),str(path),'--width',str(width),*flags,'--export-html',str(html),'--export-svg',str(svg)]
     r=subprocess.run(cmd,capture_output=True,env=env,timeout=10)
     outputs.append((r.returncode,r.stdout,r.stderr,html.read_bytes() if html.exists() else b'',svg.read_bytes() if svg.exists() else b''))
     for out in [html,svg]:
      out.unlink(missing_ok=True)
    assert outputs[0]==outputs[1],(name,width,flags)
    results.append({'fixture':name,'width':width,'flags':flags,'exit_code':outputs[1][0],'stdout_sha256':hashlib.sha256(outputs[1][1]).hexdigest(),'html_sha256':hashlib.sha256(outputs[1][3]).hexdigest(),'svg_sha256':hashlib.sha256(outputs[1][4]).hexdigest()})
args.output.write_text(json.dumps(results,indent=2)+'\n')
print(f'{len(results)} combinations match baseline stdout/stderr/status/HTML/SVG exactly')
