/**
 * Strip tmux/xterm capability probes that leak into the visible prompt when
 * the browser terminal is proxied through `tmux attach`.
 */

/** Alternate-screen CSI — disables xterm scrollback when left in the stream. */
const ALT_SCREEN_CSI =
  /\x1b\[\?(?:1049|1047|47|1004)[hl]/g;

/** Remove device-attribute and window-report CSI from server output. */
export function filterServerOutput(
  data: string,
  opts?: { preserveAltScreen?: boolean },
): string {
  let out = data;
  if (!opts?.preserveAltScreen) {
    out = out.replace(ALT_SCREEN_CSI, '');
  }
  return (
    out
      // Device attributes (primary / secondary)
      .replace(/\x1b\[[\x3f>?]?[0-9;]*c/g, '')
      .replace(/\x1b\][^\x07]*\x07/g, '')
      // Cursor position report
      .replace(/\x1b\[[0-9;]*R/g, '')
      // Window manipulation reports
      .replace(/\x1b\[[0-9;]*t/g, '')
      // Occasionally echoed as printable (^[[?1;2c)
      .replace(/\^\[\[[\x3f>?]?[0-9;]*c/g, '')
      // Bare fragments when ESC was consumed upstream
      .replace(/\?1;2c/g, '')
      .replace(/>0;\d+;\d+c/g, '')
      .replace(/1;2c0;\d+;\d+c/g, '')
  );
}

/** Drop xterm→host CSI probes; keep normal keys and pasted text. */
export function filterClientInput(data: string): string {
  if (!data.includes('\x1b')) {
    return data;
  }
  return data
    .replace(/\x1b\[[0-9;]*c/g, '')
    .replace(/\x1b\[>[0-9;]*c/g, '')
    .replace(/\x1b\[[0-9;]*[tTu]/g, '')
    .replace(/\x1b\[6n/g, '')
    .replace(/\x1bc/g, '');
}
