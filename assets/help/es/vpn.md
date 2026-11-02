---
title: VPN
section: system
order: 61
keywords: vpn, wireguard, openvpn, ipsec, ikev2, pptp, openconnect, anyconnect, fortinet
---
# VPN

En [Ajustes → Red](settings:network), la fila **VPN**:

- **Importar** una configuración: **WireGuard** (`.conf`), **OpenVPN** (`.ovpn`, con sus
  certificados al lado), **IPsec/IKEv2**, **PPTP**, y opcionalmente **OpenConnect** (AnyConnect)
  y **Fortinet**, que se activan con un botón (se instalan solos).
- **Conectar** y **desconectar** con el interruptor. Se ve el estado, la dirección y el tráfico.
- **Túnel completo o parcial:** según la configuración, va todo el tráfico por la VPN o solo el de
  ciertas redes. La ruta normal del equipo nunca se toca: al desconectar todo vuelve a como estaba.

Importar o borrar una VPN necesita ser administrador; conectar y desconectar, no. Las claves y
contraseñas quedan guardadas solo para root. **PPTP es inseguro**: está para redes viejas y se
marca como tal.
