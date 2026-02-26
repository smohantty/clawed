Name:           clawed
Version:        0.1.0
Release:        1
Summary:        Minimal self-sufficient Rust chat agent
License:        MIT
Source0:        %{name}-%{version}.tar.gz

# Smack manifest for Tizen security
%define smack_manifest %{name}.manifest

BuildRequires:  rust
BuildRequires:  cargo
BuildRequires:  openssl-devel
BuildRequires:  pkgconfig(openssl)

# Tizen writable data area
%define clawed_datadir  /opt/usr/data/%{name}

%description
Clawed is a minimal self-sufficient Rust chat agent with LLM provider
abstraction, tool execution, and a skill system. Supports Anthropic,
OpenAI, and Gemini backends.

%prep
%setup -q -n %{name}-%{version}

%build
export CARGO_HOME="$(pwd)/.cargo-home"
cargo build --release --features tizen %{?_smp_mflags:--jobs %{_smp_mflags}}

%install
# Binary
install -D -m 0755 target/release/clawed %{buildroot}%{_bindir}/clawed

# Smack manifest
install -D -m 0644 packaging/%{smack_manifest} %{buildroot}%{_datadir}/%{name}/%{smack_manifest}

# Read-only bundled skills (ship defaults here if any)
install -d %{buildroot}%{_datadir}/%{name}/skills

# Environment config
install -D -m 0644 packaging/clawed.conf %{buildroot}%{_sysconfdir}/%{name}/clawed.conf

# Writable runtime directories (skills, logs, history)
install -d %{buildroot}%{clawed_datadir}
install -d %{buildroot}%{clawed_datadir}/skills
install -d %{buildroot}%{clawed_datadir}/logs

%post
# Ensure writable dirs exist with correct ownership after install
mkdir -p %{clawed_datadir}/skills
mkdir -p %{clawed_datadir}/logs

# Copy bundled default skills into writable area if not already present
if [ -d %{_datadir}/%{name}/skills ] && [ "$(ls -A %{_datadir}/%{name}/skills 2>/dev/null)" ]; then
    cp -n %{_datadir}/%{name}/skills/* %{clawed_datadir}/skills/ 2>/dev/null || :
fi

%files
%manifest packaging/%{smack_manifest}
%{_bindir}/clawed
%{_sysconfdir}/%{name}/clawed.conf
%dir %{_datadir}/%{name}
%dir %{_datadir}/%{name}/skills
%{_datadir}/%{name}/%{smack_manifest}
%dir %{clawed_datadir}
%dir %{clawed_datadir}/skills
%dir %{clawed_datadir}/logs

%changelog
