# Copr's "make srpm" source method runs this inside its disposable source builder.
.PHONY: srpm
srpm:
	test -n "$(outdir)"
	dnf install -y git rpm-build systemd-rpm-macros nodejs npm python3
	python3 tools/desktop/srpm.py --outdir "$(outdir)"
