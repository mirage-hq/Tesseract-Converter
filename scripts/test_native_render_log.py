"""Offline provenance regressions; no Adobe process is launched."""
import pytest

import adobe_native


def test_reads_actual_hash_bound_log_not_cli_stdout(tmp_path):
    log = tmp_path / 'aerender.log'
    data = b'WARNING: missing footage\nPROGRESS: done\n'
    log.write_bytes(data)
    artifact = {'metadata': {'render_log': {'path': str(log), 'sha256': adobe_native.sha256(log)}}}
    assert adobe_native.read_render_log(artifact) == data
    log.write_bytes(b'PROGRESS: done\n')
    with pytest.raises(adobe_native.NativeAdobeError, match='changed'):
        adobe_native.read_render_log(artifact)


@pytest.mark.parametrize('metadata', [{}, {'render_log': {}}, {'render_log': {'path': 'relative.log'}}])
def test_missing_log_identity_cannot_be_replaced_by_cli_json(metadata):
    with pytest.raises(adobe_native.NativeAdobeError):
        adobe_native.read_render_log({'metadata': metadata})


def test_symlink_and_deleted_native_logs_fail_closed(tmp_path):
    log = tmp_path / 'native.log'
    log.write_bytes(b'original')
    link = tmp_path / 'link.log'
    link.symlink_to(log)
    ref = {'path': str(link), 'sha256': adobe_native.sha256(log)}
    with pytest.raises(adobe_native.NativeAdobeError, match='unavailable'):
        adobe_native.read_render_log({'metadata': {'render_log': ref}})
    ref['path'] = str(log)
    log.unlink()
    with pytest.raises(adobe_native.NativeAdobeError, match='unavailable'):
        adobe_native.read_render_log({'metadata': {'render_log': ref}})
