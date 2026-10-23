"""In-house measurement media (no corpus bytes): 1920x1080, four 270-row bands:
0 grey ramp floor(x*256/1920); 1 hue sweep S=1 V=1; 2 hue sweep S=0.5 V=0.75; 3 saturation sweep hue 30deg V=0.9."""
import colorsys, subprocess, sys
W,H=1920,1080
rows=[]
def band(fn):
    return bytes(b for x in range(W) for b in fn(x))
g=band(lambda x:(x*256//W,)*3)
def hsv(h,s,v): return tuple(round(c*255) for c in colorsys.hsv_to_rgb(h,s,v))
h1=band(lambda x:hsv(x/W,1,1))
h2=band(lambda x:hsv(x/W,0.5,0.75))
s3=band(lambda x:hsv(30/360,x/(W-1),0.9))
data=b''.join([g]*270+[h1]*270+[h2]*270+[s3]*270)
open('/tmp/d_ramp.ppm','wb').write(b'P6 %d %d 255\n'%(W,H)+data)
subprocess.check_call(['ffmpeg','-v','error','-y','-i','/tmp/d_ramp.ppm','-pix_fmt','rgb24',sys.argv[1]])
