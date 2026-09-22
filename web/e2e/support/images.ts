import jpeg from 'jpeg-js';

/**
 * A small JPEG whose content, and so BLAKE3 hash, depends on `seed`: the
 * indexer treats identical bytes as one photograph. A bright corner marks the
 * top-left so a rotation is visible in the pixels, not just the dimensions.
 */
export function makeJpeg(width: number, height: number, seed: number): Buffer {
  const data = Buffer.alloc(width * height * 4);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4;
      data[i] = (x * 255) / width + seed * 37;
      data[i + 1] = (y * 255) / height + seed * 91;
      data[i + 2] = 128 + ((seed * 53) % 100);
      data[i + 3] = 255;
      if (x < 24 && y < 24) {
        data[i] = 255;
        data[i + 1] = 255;
        data[i + 2] = 255;
      }
    }
  }
  return jpeg.encode({ data, width, height }, 85).data;
}
