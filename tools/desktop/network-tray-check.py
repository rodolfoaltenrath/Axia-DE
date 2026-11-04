#!/usr/bin/python3
"""Exercise Rust NetworkManager/SNI contracts on an isolated session bus.

Build the Rust tests first, then run with host PyGObject:
  dbus-run-session -- python3 tools/desktop/network-tray-check.py
No real network, password, or running application is changed.
"""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from gi.repository import Gio, GLib

ROOT = Path(__file__).resolve().parents[2]
NM = 'org.freedesktop.NetworkManager'
BASE = '/org/freedesktop/NetworkManager'
DEVICE = BASE + '/Devices/1'
PROFILE = BASE + '/Settings/1'
ACTIVE = BASE + '/ActiveConnection/1'
SNI = 'org.kde.StatusNotifierItem'
WATCH = 'org.kde.StatusNotifierWatcher'


def main():
    if not os.environ.get('DBUS_SESSION_BUS_ADDRESS'):
        raise SystemExit('Run this check through dbus-run-session.')
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
                  'org.freedesktop.DBus', 'RequestName',
                  GLib.Variant('(su)', (NM, 4)), None, 0, 1000, None)
    records = set()
    failures = []
    registered = False
    wired = False
    ssid = list('Café: casa'.encode())
    saved = {'connection': {'id': GLib.Variant('s', 'Perfil preservado'),
                            'type': GLib.Variant('s', '802-11-wireless')},
             '802-11-wireless': {'ssid': GLib.Variant('ay', ssid)},
             '802-11-wireless-security': {'key-mgmt': GLib.Variant('s', 'wpa-psk')},
             'ipv4': {'method': GLib.Variant('s', 'manual')}}

    def props(path, interface):
        if path == BASE and interface == NM:
            return {'WirelessEnabled': GLib.Variant('b', True),
                    'WirelessHardwareEnabled': GLib.Variant('b', True),
                    'Connectivity': GLib.Variant('u', 4),
                    'Devices': GLib.Variant('ao', [DEVICE, BASE + '/Devices/2']),
                    'PrimaryConnection': GLib.Variant('o', BASE + '/ActiveConnection/2' if wired else ACTIVE)}
        if path == BASE + '/Devices/2' and interface == NM + '.Device':
            return {'DeviceType': GLib.Variant('u', 1), 'State': GLib.Variant('u', 100 if wired else 20),
                    'ActiveConnection': GLib.Variant('o', BASE + '/ActiveConnection/2')}
        if path == DEVICE and interface == NM + '.Device':
            return {'DeviceType': GLib.Variant('u', 2),
                    'State': GLib.Variant('u', 100),
                    'ActiveConnection': GLib.Variant('o', ACTIVE),
                    'AvailableConnections': GLib.Variant('ao', [PROFILE])}
        if path == DEVICE and interface == NM + '.Device.Wireless':
            return {'AccessPoints': GLib.Variant('ao', [BASE + '/AccessPoint/' + str(i) for i in range(1, 4)]),
                    'ActiveAccessPoint': GLib.Variant('o', BASE + '/AccessPoint/1')}
        if path.startswith(BASE + '/AccessPoint/'):
            index = int(path.rsplit('/', 1)[1])
            return {'Ssid': GLib.Variant('ay', ssid),
                    'Flags': GLib.Variant('u', int(index != 3)),
                    'WpaFlags': GLib.Variant('u', 0),
                    'RsnFlags': GLib.Variant('u', 0x100 if index != 3 else 0),
                    'Strength': GLib.Variant('y', {1: 81, 2: 95, 3: 40}[index])}
        if path == ACTIVE:
            return {'State': GLib.Variant('u', 2)}
        return {}

    def call(connection, sender, path, interface, method, args, invocation):
        nonlocal wired
        try:
            if method in ('TestWired', 'TestWifiOnly'):
                wired = method == 'TestWired'
                records.add('wired' if wired else 'unplug')
                invocation.return_value(None)
                return
            if interface == 'org.freedesktop.DBus.Properties':
                values = args.unpack()
                if method == 'GetAll':
                    invocation.return_value(GLib.Variant('(a{sv})', (props(path, values[0]),)))
                elif method == 'Set':
                    assert values == (NM, 'WirelessEnabled', False)
                    records.add('toggle')
                    invocation.return_value(None)
            elif method == 'GetSettings':
                invocation.return_value(GLib.Variant('(a{sa{sv}})', (saved,)))
            elif method == 'Update':
                settings = args.unpack()[0]
                assert settings['ipv4']['method'] == 'manual'
                assert settings['connection']['id'] == 'Perfil preservado'
                assert settings['802-11-wireless-security']['psk'] == 'fixture-password'
                assert settings['802-11-wireless-security']['psk-flags'] == 0
                records.add('saved-password-update')
                invocation.return_value(None)
            elif method == 'RequestScan':
                assert args.get_type_string() == '(a{sv})'
                records.add('scan')
                invocation.return_value(None)
            elif method == 'ActivateConnection':
                assert args.unpack() == (PROFILE, DEVICE, BASE + '/AccessPoint/1')
                records.add('saved-activation')
                invocation.return_value(GLib.Variant('(o)', (ACTIVE,)))
            elif method == 'AddAndActivateConnection':
                settings, device, ap = args.unpack()
                assert bytes(settings['802-11-wireless']['ssid']).decode() == 'Nova: rede'
                assert settings['802-11-wireless-security']['key-mgmt'] == 'wpa-psk'
                assert settings['802-11-wireless-security']['psk'] == 'fixture-password'
                assert device == DEVICE and ap == BASE + '/AccessPoint/1'
                records.add('new-activation')
                invocation.return_value(GLib.Variant('(oo)', (PROFILE, ACTIVE)))
            elif method == 'Denied':
                invocation.return_dbus_error(NM + '.PermissionDenied', 'fixture denied')
                records.add('denied')
            else:
                raise AssertionError(method)
        except Exception as error:
            failures.append(f'{method}: {type(error).__name__}')
            invocation.return_dbus_error('org.axia.Test.Failed', type(error).__name__)

    xml = '''<node><interface name="org.freedesktop.DBus.Properties">
    <method name="GetAll"><arg type="s" direction="in"/><arg type="a{sv}" direction="out"/></method>
    <method name="Set"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/></method></interface>
    <interface name="org.freedesktop.NetworkManager">
    <method name="ActivateConnection"><arg type="o" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="out"/></method>
    <method name="AddAndActivateConnection"><arg type="a{sa{sv}}" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="in"/><arg type="o" direction="out"/><arg type="o" direction="out"/></method>
    <method name="Denied"/><method name="TestWired"/><method name="TestWifiOnly"/></interface>
    <interface name="org.freedesktop.NetworkManager.Device.Wireless"><method name="RequestScan"><arg type="a{sv}" direction="in"/></method></interface>
    <interface name="org.freedesktop.NetworkManager.Settings.Connection"><method name="GetSettings"><arg type="a{sa{sv}}" direction="out"/></method><method name="Update"><arg type="a{sa{sv}}" direction="in"/></method></interface></node>'''
    info = Gio.DBusNodeInfo.new_for_xml(xml)
    for path in [BASE, DEVICE, BASE + '/Devices/2', PROFILE, ACTIVE] + [BASE + '/AccessPoint/' + str(i) for i in range(1, 4)]:
        for interface in info.interfaces:
            bus.register_object(path, interface, call, None, None)

    layout = (0, {}, [GLib.Variant('(ia{sv}av)', (1, {'label': GLib.Variant('s', '_Abrir janela')}, [])),
                      GLib.Variant('(ia{sv}av)', (2, {'label': GLib.Variant('s', '_Sair')}, []))])
    def tray_call(connection, sender, path, interface, method, args, invocation):
        try:
            if method == 'Activate':
                assert args.unpack() == (12, 34)
                records.add('tray-activate')
                invocation.return_value(None)
            elif method == 'AboutToShow':
                records.add('tray-about-to-show')
                invocation.return_value(GLib.Variant('(b)', (False,)))
            elif method == 'GetLayout':
                assert args.unpack() == (0, 4, [])
                records.add('tray-menu-layout')
                invocation.return_value(GLib.Variant('(u(ia{sv}av))', (1, layout)))
            elif method == 'Event':
                assert args.unpack() == (2, 'clicked', 0, 0)
                records.add('tray-menu-event')
                invocation.return_value(None)
                GLib.idle_add(release_item)
            else:
                raise AssertionError(method)
        except Exception as error:
            failures.append(f'{method}: {type(error).__name__}')
            invocation.return_dbus_error('org.axia.Test.Failed', type(error).__name__)

    # Use a separate connection for the app, so closing it tests cleanup.
    app = Gio.DBusConnection.new_for_address_sync(os.environ['DBUS_SESSION_BUS_ADDRESS'],
        Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
    def release_item():
        app.flush_sync(None)
        app.close_sync(None)
        records.add('tray-disconnect')
        return False
    def tray_prop(connection, sender, path, interface, name):
        return {'Id': GLib.Variant('s', 'discord-test'),
                'Title': GLib.Variant('s', 'Discord de teste'),
                'Status': GLib.Variant('s', 'Active'),
                'Menu': GLib.Variant('o', '/Menu'),
                'ItemIsMenu': GLib.Variant('b', False),
                'IconName': GLib.Variant('s', ''),
                'IconPixmap': GLib.Variant('a(iiay)', [(2, 2, [255, 90, 100, 240] * 4)])}[name]
    tray_xml = '''<node><interface name="org.kde.StatusNotifierItem">
    <method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <property name="Id" type="s" access="read"/><property name="Title" type="s" access="read"/>
    <property name="Status" type="s" access="read"/><property name="Menu" type="o" access="read"/>
    <property name="ItemIsMenu" type="b" access="read"/><property name="IconName" type="s" access="read"/>
    <property name="IconPixmap" type="a(iiay)" access="read"/></interface>
    <interface name="com.canonical.dbusmenu"><method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
    <method name="GetLayout"><arg type="i" direction="in"/><arg type="i" direction="in"/><arg type="as" direction="in"/><arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/></method>
    <method name="Event"><arg type="i" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/><arg type="u" direction="in"/></method></interface></node>'''
    tray_info = Gio.DBusNodeInfo.new_for_xml(tray_xml)
    app.register_object('/StatusNotifierItem', tray_info.interfaces[0], tray_call, tray_prop, None)
    app.register_object('/Menu', tray_info.interfaces[1], tray_call, None, None)

    def register_app():
        nonlocal registered
        if registered:
            return False
        try:
            reply = bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
                'org.freedesktop.DBus', 'NameHasOwner', GLib.Variant('(s)', (WATCH,)), None, 0, 1000, None)
            if reply.unpack()[0]:
                app.call(WATCH, '/StatusNotifierWatcher', WATCH, 'RegisterStatusNotifierItem',
                         GLib.Variant('(s)', ('/StatusNotifierItem',)), None, 0, 1000, None, None)
                registered = True
        except GLib.Error:
            pass
        return not registered
    GLib.timeout_add(40, register_app)
    binaries = [p for p in (ROOT / 'server/build/rust/debug/deps').glob('axia_shell-*') if p.is_file() and os.access(p, os.X_OK) and p.suffix == '']
    binary = max(binaries, key=lambda p: p.stat().st_mtime)
    with tempfile.TemporaryDirectory(prefix='axia-network-tray-check-') as tmp:
        env = {**os.environ, 'DBUS_SYSTEM_BUS_ADDRESS': os.environ['DBUS_SESSION_BUS_ADDRESS'],
               'AXIA_PRIVATE_BUS_TEST': '1', 'XDG_CACHE_HOME': tmp}
        process = subprocess.Popen([str(binary), 'desktop_bus_contract', '--ignored', '--nocapture'], env=env,
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        deadline = time.monotonic() + 30
        context = GLib.MainContext.default()
        while process.poll() is None and time.monotonic() < deadline:
            while context.pending():
                context.iteration(False)
            time.sleep(.01)
        if process.poll() is None:
            process.kill()
        output = process.communicate()[0]
        print(output, end='')
        expected = {'scan', 'toggle', 'saved-password-update', 'saved-activation', 'new-activation', 'denied', 'wired', 'unplug',
                    'tray-activate', 'tray-about-to-show', 'tray-menu-layout', 'tray-menu-event', 'tray-disconnect'}
        missing = expected - records
        if process.returncode or failures or missing:
            raise SystemExit(f'FAILED: exit={process.returncode}, errors={failures}, missing={sorted(missing)}')
        print('Network and tray private-bus checks passed; host connectivity untouched.')


if __name__ == '__main__':
    # Always create our own bus, even if invoked directly from a real desktop.
    if sys.argv[1:] != ['--private']:
        raise SystemExit(subprocess.call(['dbus-run-session', '--', sys.executable, str(Path(__file__).resolve()), '--private']))
    main()
