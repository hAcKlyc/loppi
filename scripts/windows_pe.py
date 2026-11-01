"""Read PE architecture and imports without loading or executing the binary."""
import json, struct, sys
from pathlib import Path

def inspect(path):
    data=Path(path).read_bytes()
    pe=struct.unpack_from('<I',data,0x3c)[0]
    if data[pe:pe+4]!=b'PE\0\0': raise ValueError(f'not a PE image: {path}')
    machine,sections=struct.unpack_from('<HH',data,pe+4)
    optional=pe+24
    magic=struct.unpack_from('<H',data,optional)[0]
    directories=optional+(112 if magic==0x20b else 96)
    import_rva=struct.unpack_from('<I',data,directories+8)[0]
    delay_rva=struct.unpack_from('<I',data,directories+13*8)[0]
    section_size=struct.unpack_from('<H',data,pe+20)[0]
    table=optional+section_size
    def offset(rva):
        for index in range(sections):
            entry=table+40*index
            size,virtual,raw_size,raw=struct.unpack_from('<IIII',data,entry+8)
            if virtual<=rva<virtual+max(size,raw_size):return raw+rva-virtual
        raise ValueError('RVA outside PE sections')
    imports=[]
    if import_rva:
        entry=offset(import_rva)
        while any(data[entry:entry+20]):
            name=offset(struct.unpack_from('<I',data,entry+12)[0])
            imports.append(data[name:data.index(b'\0',name)].decode('ascii'))
            entry+=20
    delay_imports=[]
    if delay_rva:
        entry=offset(delay_rva)
        image_base=struct.unpack_from('<Q' if magic==0x20b else '<I',data,optional+(24 if magic==0x20b else 28))[0]
        while any(data[entry:entry+32]):
            attributes,name=struct.unpack_from('<II',data,entry)
            name=offset(name if attributes&1 else name-image_base)
            delay_imports.append(data[name:data.index(b'\0',name)].decode('ascii'));entry+=32
    return {'path':str(path),'machine':hex(machine),'architecture':{0x8664:'x64',0xaa64:'arm64',0x14c:'x86'}.get(machine,'unknown'),'imports':imports,'delay_imports':delay_imports}

if __name__=='__main__':
    print(json.dumps([inspect(Path(path)) for path in sys.argv[1:]],indent=2))
