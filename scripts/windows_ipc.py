"""Test client for the product's restricted named-pipe framing (no network listener)."""
import ctypes, hashlib, json, msvcrt, os, struct, time, uuid
from ctypes import wintypes
from pathlib import Path

kernel=ctypes.WinDLL('kernel32',use_last_error=True)
advapi=ctypes.WinDLL('advapi32',use_last_error=True)
kernel.GetCurrentProcess.restype=wintypes.HANDLE
kernel.CloseHandle.argtypes=[wintypes.HANDLE]
kernel.ProcessIdToSessionId.argtypes=[wintypes.DWORD,ctypes.POINTER(wintypes.DWORD)]
advapi.OpenProcessToken.argtypes=[wintypes.HANDLE,wintypes.DWORD,ctypes.POINTER(wintypes.HANDLE)]
advapi.GetTokenInformation.argtypes=[wintypes.HANDLE,ctypes.c_int,ctypes.c_void_p,wintypes.DWORD,ctypes.POINTER(wintypes.DWORD)]
advapi.GetLengthSid.argtypes=[ctypes.c_void_p];advapi.GetLengthSid.restype=wintypes.DWORD
kernel.CreateFileW.argtypes=[wintypes.LPCWSTR,wintypes.DWORD,wintypes.DWORD,ctypes.c_void_p,wintypes.DWORD,wintypes.DWORD,wintypes.HANDLE];kernel.CreateFileW.restype=wintypes.HANDLE

def pipe_name(directory):
    token=wintypes.HANDLE();assert advapi.OpenProcessToken(kernel.GetCurrentProcess(),8,ctypes.byref(token))
    size=wintypes.DWORD();advapi.GetTokenInformation(token,1,None,0,ctypes.byref(size));buffer=ctypes.create_string_buffer(size.value)
    assert advapi.GetTokenInformation(token,1,buffer,size,ctypes.byref(size));kernel.CloseHandle(token)
    sid=ctypes.c_void_p.from_buffer(buffer).value;user=ctypes.string_at(sid,advapi.GetLengthSid(sid))
    session=wintypes.DWORD();assert kernel.ProcessIdToSessionId(os.getpid(),ctypes.byref(session))
    path=str(Path(directory).resolve())
    if not path.startswith('\\\\?\\'):path='\\\\?\\'+path
    digest=hashlib.sha256(user+path.lower().encode('utf8')).hexdigest()
    return '\\\\.\\pipe\\loppi-'+str(session.value)+'-'+digest

def connect(directory):
    name=pipe_name(directory);deadline=time.monotonic()+5
    while True:
        # SecurityIdentification limits the server's impersonation rights.
        handle=kernel.CreateFileW(name,0xc0000000,0,None,3,0x00100000|0x00010000,None)
        if handle!=wintypes.HANDLE(-1).value:break
        error=ctypes.get_last_error()
        if time.monotonic()>=deadline:raise OSError(error,'cannot open product pipe',name)
        time.sleep(.025)
    return os.fdopen(msvcrt.open_osfhandle(handle,os.O_RDWR|os.O_BINARY),'r+b',buffering=0)

def request(directory,body,client='cli'):
    identity=str(uuid.uuid4());payload=json.dumps({'version':24,'id':identity,'client':client,'request':body}).encode()
    with connect(directory) as stream:
        stream.write(struct.pack('<I',len(payload))+payload)
        def exact(length):
            data=b''
            while len(data)<length:
                part=stream.read(length-len(data))
                if not part:raise EOFError('truncated pipe frame')
                data+=part
            return data
        size=struct.unpack('<I',exact(4))[0]
        if size>48*1024*1024:raise ValueError('response limit')
        value=json.loads(exact(size));assert value['id']==identity,value
        return value
