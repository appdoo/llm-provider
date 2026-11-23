# Security policy

To report a vulnerability, use GitHub's private vulnerability reporting: the **Security** tab of [appdoo/llm-provider](https://github.com/appdoo/llm-provider/security), then "Report a vulnerability". Please do not open a public issue for it.

Say which version you use and how to reproduce the problem. Fixes go into the latest 0.x release.

Every release passes `scripts/security.sh` (`cargo deny`, `osv-scanner`, `gitleaks`, `semgrep`).
