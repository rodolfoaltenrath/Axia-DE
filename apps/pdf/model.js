// Shared transport for PDF geometry and cache metadata. A QuickJS finalizer
// owns each Rust model; JS image handles and promises never cross this boundary.
export function createPdfModel() {
  const owner = lord.nativeApps.create('pdf');
  return value => {
    const reply = owner.request(JSON.stringify(value));
    if (reply?.error) throw new Error(reply.error);
    return reply;
  };
}
