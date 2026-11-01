"""Synthetic PNG OCR evidence only. Does not capture the desktop or claim HWND exclusion."""
import hashlib,json,os,subprocess,shutil
from pathlib import Path
from PIL import Image,ImageDraw,ImageFont

root=Path.cwd();directory=root/'build/windows-ocr-test';directory.mkdir(parents=True,exist_ok=True)
runtime=directory/'runtime 🐰 中文 space';shutil.copytree(root/'build/cache/windows/ocr-runtime',runtime,dirs_exist_ok=True);image=Image.new('RGB',(1800,1000),'white');draw=ImageDraw.Draw(image)
font=ImageFont.truetype('C:/Windows/Fonts/arial.ttf',65)
draw.text((80,180),'LOPPI WINDOWS OCR 12345',font=font,fill='black');draw.text((80,350),'Synthetic offline OCR acceptance',font=font,fill='black')
draw.text((80,520),'离线文字识别测试',font=ImageFont.truetype('C:/Windows/Fonts/msyh.ttc',65),fill='black')
path=directory/'合成 🌟 image.png';image.save(path)
env=os.environ.copy();env.update(LOPPI_TEST_OCR_RUNTIME=str(runtime),LOPPI_TEST_OCR_PNG=str(path),CARGO_PROFILE_DEV_DEBUG='0',CARGO_INCREMENTAL='0',SHERPA_ONNX_LIB_DIR=str(root/'build/cache/windows/sherpa-onnx-v2.0.0-win-x64-shared-MD-Release-lib/lib'))
result=subprocess.run(['cargo','test','-p','loppi-core','ocr::tests::recognizes_synthetic_png_with_bundled_engine','--','--ignored','--exact'],env=env,timeout=300)
report={'schema':1,'passed':result.returncode==0,'desktop_capture':False,'fixture':'locally rendered synthetic PNG','engine_sha256':hashlib.sha256((runtime/'tesseract.exe').read_bytes()).hexdigest()}
(directory/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf8');raise SystemExit(result.returncode)
