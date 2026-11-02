---
title: VPN
section: system
order: 61
keywords: vpn, wireguard, openvpn, ipsec, ikev2, pptp, openconnect, anyconnect, fortinet
---
# VPN

Em [Ajustes → Rede](settings:network), a linha **VPN**:

- **Importar** uma configuração: **WireGuard** (`.conf`), **OpenVPN** (`.ovpn`, com os
  certificados ao lado), **IPsec/IKEv2**, **PPTP**, e opcionalmente **OpenConnect** (AnyConnect)
  e **Fortinet**, ativados com um botão (instalam-se sozinhos).
- **Conectar** e **desconectar** com o interruptor. Aparecem o estado, o endereço e o tráfego.
- **Túnel completo ou parcial:** conforme a configuração, todo o tráfego vai pela VPN ou só o de
  certas redes. A rota normal do computador nunca é alterada: ao desconectar, tudo volta a ser como
  era.

Importar ou apagar uma VPN exige ser administrador; conectar e desconectar, não. As chaves e senhas
ficam guardadas só para o root. **PPTP é inseguro**: existe para redes antigas e está marcado assim.
