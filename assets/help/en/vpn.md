---
title: VPN
section: system
order: 61
keywords: vpn, wireguard, openvpn, ipsec, ikev2, pptp, openconnect, anyconnect, fortinet
---
# VPN

In [Settings → Network](settings:network), the **VPN** row:

- **Import** a configuration: **WireGuard** (`.conf`), **OpenVPN** (`.ovpn`, with its certificates
  next to it), **IPsec/IKEv2**, **PPTP**, and optionally **OpenConnect** (AnyConnect) and
  **Fortinet**, enabled with a button (they install themselves).
- **Connect** and **disconnect** with the switch. You see the status, address and traffic.
- **Full or split tunnel:** depending on the configuration, all traffic goes through the VPN or
  just certain networks. The computer's normal route is never touched: when you disconnect,
  everything goes back to how it was.

Importing or deleting a VPN requires an administrator; connecting and disconnecting don't. Keys and
passwords are stored readable only by root. **PPTP is insecure**: it is there for old networks and
marked as such.
