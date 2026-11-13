// Image results remain in their window lease after delivery. Rust only sees
// handles; disposing an obsolete request or closing its window releases them.
export function documentImageService(fn, releaseImage) {
  return {
    async run(args, owner) {
      const result = await fn(...args);
      owner.retain(`image:${result.image}`, result.image, releaseImage);
      return result;
    },
    dispose(result, owner) { if (result?.image) owner.release(`image:${result.image}`); },
  };
}
export function documentServices({ decodeImage, convertImage, pdfThumb, releaseImage }) {
  return {
    'images.decode': documentImageService(decodeImage, releaseImage),
    'images.pdfThumb': documentImageService(pdfThumb, releaseImage),
    'images.convert': { run: args => convertImage(...args) },
    'images.release': { run: ([ids], owner) => { for (const id of ids) owner.release(`image:${id}`); } },
  };
}
