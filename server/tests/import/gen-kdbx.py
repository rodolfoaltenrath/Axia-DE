# KDBX 4 with pykeepass (other cipher/KDF/key file), same entries as sample.xml.
from pykeepass import PyKeePass, create_database
import copy, datetime
PW = 'lordos-prueba'
open('kx-keyfile.key', 'wb').write(bytes(range(64)))
open('kx-keyfile2.keyx', 'w').write('''<?xml version="1.0" encoding="UTF-8"?>
<KeyFile>
    <Meta>
        <Version>2.0</Version>
    </Meta>
    <Key>
        <Data Hash="FE2949B8">
            A7007945 D07D54BA 28DF6434 1B4500FC
            9750DFB1 D36ADA2D 9C32DC19 4C7AB01B
        </Data>
    </Key>
</KeyFile>
''')
k = create_database('kx4-base.kdbx', password=PW)
print('base', k.version, k.encryption_algorithm, k.kdf_algorithm)
inet = k.add_group(k.root_group, 'Internet')
bancos = k.add_group(inet, 'Bancos')
srv = k.add_group(k.root_group, 'Servidores')
e = k.add_entry(bancos, 'Banco Ñandú', 'juan.pérez', 's3cr3t&"<€', url='https://banco.example.com/login',
                notes='línea 1\nlínea 2', tags=['finanzas', 'importante'],
                otp='otpauth://totp/Banco:juan?secret=JBSWY3DPEHPK3PXP&period=30&digits=6&issuer=Banco')
e.set_custom_property('PIN', '4321', protect=True)
e.set_custom_property('Cliente', '00012345')
bid = k.add_binary(b'Hola, adjunto de prueba!\n')
e.add_attachment(bid, 'adjunto.txt')
e.ctime = datetime.datetime(2024, 3, 5, 10, 20, 30, tzinfo=datetime.timezone.utc)
e.mtime = datetime.datetime(2025, 1, 2, 3, 4, 5, tzinfo=datetime.timezone.utc)
k.add_entry(inet, 'Correo', 'yo@example.org', 'correo-123', url='https://mail.example.org', tags=['web'])
k.add_entry(srv, 'vps1', 'root', 'ssh-pass', url='ssh://203.0.113.5:22')
k.add_entry(k.root_group, 'Raíz suelta', 'u', 'p')
# an entry in the trash: it is not imported
trash = k.add_entry(k.root_group, 'Borrada', 'x', 'y')
k.trash_entry(trash)
d0 = k.kdbx.header.value.dynamic_header.kdf_parameters.data.dict
print(d0)
d0['I'].value = 2; d0['M'].value = 1024 * 1024; d0['P'].value = 1
k.save()
def variant(out, cipher, kdf, keyfile=None):
    kp = PyKeePass('kx4-base.kdbx', password=PW)
    h = kp.kdbx.header.value.dynamic_header
    h.cipher_id.data = cipher
    d = h.kdf_parameters.data.dict
    if kdf == 'argon2id':
        d['$UUID'].value = bytes.fromhex('9e298b1956db4773b23dfc3ec6f0a1e6')
    elif kdf == 'argon2d':
        d['$UUID'].value = bytes.fromhex('ef636ddf8c29444b91f7a9a403e30a0c')
    elif kdf == 'aeskdf':
        d['$UUID'].value = bytes.fromhex('c9d9f39a628a4460bf740d08c18a4fea')
        from construct import Container
        def cp(c, **kw):
            n = Container({k: v for k, v in c.items() if not k.startswith('_')})
            n.update(kw); return n
        r = cp(d['I'], key='R', value=20000, type=5, next_byte=66); d['$UUID'].next_byte = 5
        s = cp(d['S'], value=bytes(range(32)), next_byte=0)
        for key in list(d.keys()):
            if key != '$UUID': del d[key]
        d['R'] = r; d['S'] = s
    if keyfile: kp.keyfile = keyfile
    kp.save(out)
    k2 = PyKeePass(out, password=PW, keyfile=keyfile)
    print(out, k2.version, k2.encryption_algorithm, k2.kdf_algorithm, len(k2.entries))
variant('kx4-aes-argon2d.kdbx', 'aes256', 'argon2d')
variant('kx4-chacha-argon2id-key.kdbx', 'chacha20', 'argon2id', 'kx-keyfile.key')
variant('kx4-aes-argon2id-keyx.kdbx', 'aes256', 'argon2id', 'kx-keyfile2.keyx')
variant('kx4-chacha-aeskdf.kdbx', 'chacha20', 'aeskdf')
import os; os.remove('kx4-base.kdbx')
k3 = PyKeePass('kx3-aeskdf.kdbx', password=PW)
k3.kdbx.header.value.dynamic_header.transform_rounds.data = 6000
hh = k3.tree.find('Meta/HeaderHash')
if hh is not None: hh.getparent().remove(hh)
k3.save('kx3-aeskdf.kdbx')
k3 = PyKeePass('kx3-aeskdf.kdbx', password=PW)
print('v3', k3.version, k3.encryption_algorithm, len(k3.entries))
