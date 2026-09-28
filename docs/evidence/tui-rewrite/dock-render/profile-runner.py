#!/usr/bin/env python3
from pathlib import Path
import os,sys
if '--list' in sys.argv:
 os.execv(sys.argv[1],sys.argv[1:])
out=Path(__file__).resolve().parent
os.execvp('gdb',['gdb','--batch','-ex',f'cd {out}','-x',str(out/'profile.gdb'),'--args',*sys.argv[1:]])
