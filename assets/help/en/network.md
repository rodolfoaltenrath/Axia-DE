---
title: Network and Wi-Fi
section: system
order: 60
keywords: network, wifi, internet, ethernet, wired, dhcp, ip, dns
---
# Network and Wi-Fi

The network is managed in [Settings → Network](settings:network). At the top there is a row per
connection (wired, Wi-Fi and VPN); Control Center shows the main one.

- **Wired:** connects automatically with DHCP. You can also set a **static IP** (address, gateway
  and DNS).
- **Wi-Fi:** choose the network and type the password. Saved networks reconnect by themselves, and
  “Forget” removes them.
- **Status:** IP address, gateway, DNS and traffic for each connection.

The network belongs to the **system**, not to your session: it comes up when the computer boots,
before anyone logs in, and stays the same when you switch users or log out. That's why the login
screen already shows the computer's IP.

Joining a Wi-Fi network doesn't require an administrator; changing an interface's configuration
does. Network passwords are stored by the system and only root can read them. For VPNs, see
[VPN](help:vpn).
