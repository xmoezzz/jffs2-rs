#include <stdint.h>

#define RUBIN_REG_SIZE   16
#define UPPER_BIT_RUBIN    (((long) 1)<<(RUBIN_REG_SIZE-1))
#define LOWER_BITS_RUBIN   ((((long) 1)<<(RUBIN_REG_SIZE-1))-1)

void rubin_do_decompress(unsigned char *bits, unsigned char *in,
			 unsigned char *page_out, uint32_t destlen)
{
	char *curr = (char *)page_out;
	char *end = (char *)(page_out + destlen);
	uint32_t temp;
	uint32_t result;
	uint32_t p;
	uint32_t q;
	uint32_t rec_q;
	uint32_t bit;
	long i0;
	uint32_t i;

	/* init_pushpull */
	temp = *(uint32_t *) in;
	bit = 16;

	/* init_rubin */
	q = 0;
	p = (long) (2 * UPPER_BIT_RUBIN);

	/* init_decode */
	rec_q = (in[0] << 8) | in[1];

	while (curr < end) {
		/* in byte */

		result = 0;
		for (i = 0; i < 8; i++) {
			/* decode */

			while ((q & UPPER_BIT_RUBIN) || ((p + q) <= UPPER_BIT_RUBIN)) {
				q &= ~UPPER_BIT_RUBIN;
				q <<= 1;
				p <<= 1;
				rec_q &= ~UPPER_BIT_RUBIN;
				rec_q <<= 1;
				rec_q |= (temp >> (bit++ ^ 7)) & 1;
				if (bit > 31) {
					uint32_t *p = (uint32_t *)in;
					bit = 0;
					temp = *(++p);
					in = (unsigned char *)p;
				}
			}
			i0 =  (bits[i] * p) >> 8;

			if (i0 <= 0) i0 = 1;
			/* if it fails, it fails, we have our crc
			if (i0 >= p) i0 = p - 1; */

			result >>= 1;
			if (rec_q < q + i0) {
				/* result |= 0x00; */
				p = i0;
			} else {
				result |= 0x80;
				p -= i0;
				q += i0;
			}
		}
		*(curr++) = result;
	}
}

void dynrubin_decompress(unsigned char *data_in, unsigned char *cpage_out,
		   uint32_t sourcelen, uint32_t dstlen)
{
	unsigned char bits[8];
	int c;

	for (c=0; c<8; c++)
		bits[c] = (256 - data_in[c]);

	rubin_do_decompress(bits, data_in+8, cpage_out, dstlen);
}
