# Copr's "make srpm" source method runs this inside its disposable source builder.
.PHONY: srpm
srpm:
	test -n "$(outdir)"
	dnf install -y git rpm-build systemd-rpm-macros nodejs npm cargo
	cargo xtask srpm --outdir "$(outdir)"
