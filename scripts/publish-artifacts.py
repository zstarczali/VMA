"""Publish the VMA packages of a Release run and the PDF manuals over explicit FTPS (TLS on port 21).

  publish-artifacts.py --validate-run              the Release run (BUILD_RUN_ID) succeeded; its commit → GITHUB_OUTPUT sha
  publish-artifacts.py ARTIFACTS SOURCE            upload the packages (ARTIFACTS, extracted) and the PDFs of the
                                                   build commit (SOURCE), then remove the packages of older versions
"""
import ftplib
import json
import os
import re
from pathlib import Path, PurePosixPath
import ssl
import sys
import urllib.request

WORKFLOW = '.github/workflows/release.yml'
PLATFORMS = ('windows-x64', 'windows-arm64', 'macos-x64', 'macos-arm64', 'linux-x64')
PACKAGE = re.compile(r'^vma-(\d+)\.(\d+)\.(\d+)-(' + '|'.join(PLATFORMS) + r')\.(?:zip|tar\.gz)$')
# published name → path in the build commit
DOCUMENTS = {'README.pdf': 'README.pdf', 'VMA-Manual.pdf': 'docs/VMA-Manual.pdf', 'VMA-Manual.hu.pdf': 'docs/VMA-Manual.hu.pdf'}


def validate_run():
    run_id = os.environ['BUILD_RUN_ID']
    if not run_id.isascii() or not run_id.isdecimal():
        raise ValueError('Build run ID must be a positive integer.')
    repository = os.environ['GH_REPOSITORY']
    request = urllib.request.Request(
        f'https://api.github.com/repos/{repository}/actions/runs/{run_id}',
        headers={'Authorization': 'Bearer ' + os.environ['GH_TOKEN'],
                 'Accept': 'application/vnd.github+json',
                 'X-GitHub-Api-Version': '2022-11-28'},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        run = json.load(response)
    if run.get('path', '').split('@')[0] != WORKFLOW:
        raise ValueError('The selected run is not a Release run.')
    if run.get('status') != 'completed' or run.get('conclusion') != 'success':
        raise ValueError('The selected build must have completed successfully.')
    sha = run['head_sha']
    if not re.fullmatch(r'[0-9a-f]{40}', sha):
        raise ValueError('Unexpected commit of the build run.')
    output = os.environ.get('GITHUB_OUTPUT')
    if output:
        with open(output, 'a', encoding='utf-8') as out:
            out.write(f'sha={sha}\n')
    print(f'Validated build {run_id}, commit {sha}.')


def version_of(package):
    match = PACKAGE.fullmatch(package)
    return tuple(map(int, match.groups()[:3])) if match else None


def cargo_version(source):
    text = (Path(source) / 'Cargo.toml').read_text(encoding='utf-8')
    match = re.search(r'^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"', text, re.MULTILINE)
    if not match:
        raise ValueError('No version in Cargo.toml of the build commit.')
    return tuple(map(int, match.groups()))


def collect_files(root, source):
    """name → path: every platform's package of one version (the build commit's), then the PDFs."""
    packages = {}
    for path in sorted(Path(root).rglob('*')):
        if path.is_symlink():
            raise ValueError('Artifact symlinks are not supported.')
        if not path.is_file() or not PACKAGE.fullmatch(path.name):
            continue
        if path.name in packages:
            raise ValueError(f'Duplicate output filename: {path.name}')
        if path.stat().st_size == 0:
            raise ValueError(f'Empty artifact file: {path.name}')
        packages[path.name] = path
    versions = {version_of(name) for name in packages}
    if len(versions) != 1:
        raise ValueError('The packages must be of exactly one version, found: ' + (', '.join('.'.join(map(str, v)) for v in sorted(versions)) or 'none'))
    version = versions.pop()
    if version != cargo_version(source):
        raise ValueError('The packages are not of the build commit\'s Cargo.toml version.')
    platforms = sorted(PACKAGE.fullmatch(name).group(4) for name in packages)
    if platforms != sorted(PLATFORMS):
        raise ValueError('Missing or extra platform packages: ' + ', '.join(platforms))
    files = dict(packages)
    for name, rel in DOCUMENTS.items():
        path = Path(source) / rel
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f'Missing document in the build commit: {rel}')
        files[name] = path
    return files, version


def safe_error(error):
    message = str(error)
    for key in ('FTP_PASSWORD', 'GH_TOKEN'):
        secret = os.environ.get(key)
        if secret:
            message = message.replace(secret, '[REDACTED]')
    return ' '.join(message.splitlines())[:600]


def remove_previous_packages(ftp, files, current, directory):
    removed = 0
    for entry in ftp.nlst():
        path = PurePosixPath(entry)
        if len(path.parts) > 1 and path.parent != directory:
            continue
        previous = version_of(path.name)
        if path.name in files or previous is None or previous >= current:
            continue
        print(f'Removing previous package: {path.name}', flush=True)
        ftp.delete(path.name)
        removed += 1
    print(f'Removed {removed} previous packages.', flush=True)


def open_directory(ftp, directory):
    try:
        ftp.cwd(str(directory))
        return
    except ftplib.error_perm as error:
        if not str(error).startswith('550'):
            raise
    ftp.cwd('/')
    for part in directory.parts[1:]:
        try:
            ftp.cwd(part)
        except ftplib.error_perm as error:
            if not str(error).startswith('550'):
                raise
            ftp.mkd(part)
            ftp.cwd(part)


def upload(root, source):
    files, version = collect_files(root, source)
    password = os.environ.get('FTP_PASSWORD', '')
    if not password:
        raise ValueError('Set the FTP_PASSWORD repository Actions secret before publishing.')
    directory = PurePosixPath(os.environ['FTP_DIRECTORY'])
    if not directory.is_absolute() or '..' in directory.parts:
        raise ValueError('FTP_DIRECTORY must be an absolute directory without parent traversal.')
    stage = 'opening FTPS connection'
    try:
        # Validate the server certificate and encrypt both login and data connections.
        with ftplib.FTP_TLS(context=ssl.create_default_context(), timeout=120) as ftp:
            stage = 'connecting to server'
            ftp.connect(os.environ['FTP_HOST'], 21)
            stage = 'login (check Forpsi country/IP restrictions and the FTP_PASSWORD secret)'
            ftp.login(os.environ['FTP_USERNAME'], password)
            stage = 'enabling encrypted data transfers'
            ftp.prot_p()
            ftp.set_pasv(True)
            stage = f'opening target directory {directory}'
            open_directory(ftp, directory)
            # the packages first, the manuals last
            for name, path in files.items():
                stage = f'uploading {name}'
                print(f'Uploading {name} ({path.stat().st_size} bytes)', flush=True)
                with path.open('rb') as data:
                    ftp.storbinary('STOR ' + name, data, blocksize=256 * 1024)
                stage = f'verifying uploaded size of {name}'
                ftp.voidcmd('TYPE I')
                if ftp.size(name) != path.stat().st_size:
                    raise ValueError(f'Remote size verification failed: {name}')
            stage = 'removing previous packages'
            remove_previous_packages(ftp, files, version, directory)
    except (ftplib.Error, OSError, EOFError) as error:
        detail = safe_error(error)
        if stage.startswith('login') and str(error).startswith('530'):
            detail += ' Forpsi can also reject valid credentials through country/IP restrictions. Check its FTP log for DENY.'
        raise ValueError(f'FTP {stage}: {detail}') from None
    print(f'Published VMA {".".join(map(str, version))}: {len(files)} files. Existing unrelated files were retained.')


if __name__ == '__main__':
    try:
        if sys.argv[1:] == ['--validate-run']:
            validate_run()
        elif len(sys.argv) == 3:
            upload(sys.argv[1], sys.argv[2])
        else:
            raise ValueError('Usage: publish-artifacts.py ARTIFACT_DIRECTORY SOURCE_DIRECTORY | --validate-run')
    except Exception as error:
        # FTP errors can contain server responses: never print login details.
        if isinstance(error, ValueError):
            print(f'Publish failed: {error}', file=sys.stderr)
        else:
            print(f'Publish failed ({type(error).__name__}). Check the connection and source build.', file=sys.stderr)
        sys.exit(1)
