use embedded_graphics::pixelcolor::{raw::ToBytes, Rgb888};
use image::{buffer::Pixels, io::Reader as ImageReader, Pixel, Rgba, RgbaImage};
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use std::path::PathBuf;
use syn::{parse_macro_input, spanned::Spanned as _, Expr, ExprLit, ItemConst, Lit};

fn pixels_to_rgb888(pixels: Pixels<Rgba<u8>>) -> syn::Result<(Vec<u8>, Vec<u8>)> {
	let mut colors = Vec::new();
	let mut transparency = Vec::new();

	for pixel in pixels {
		let mut channels = pixel.channels().iter();
		let r = *channels.next().unwrap_or(&0);
		let g = *channels.next().unwrap_or(&0);
		let b = *channels.next().unwrap_or(&0);
		let a = *channels.next().unwrap_or(&255);

		// Create base color
		let color = Rgb888::new(r, g, b);

		// Store transparency (true if alpha is 0)
		transparency.push((a == 0) as u8);
		colors.extend_from_slice(&color.to_be_bytes());
	}
	Ok((colors, transparency))
}

fn expand(
	ItemConst {
		attrs,
		vis,
		const_token,
		ident,
		colon_token,
		ty,
		eq_token,
		expr,
		semi_token,
	}: ItemConst,
) -> syn::Result<TokenStream2> {
	let path_lit = match expr.as_ref() {
		Expr::Lit(ExprLit { lit: Lit::Str(path), .. }) => path,
		expr => return Err(syn::Error::new(expr.span(), "Expected path to image")),
	};
	let path: PathBuf = path_lit
		.value()
		.parse()
		.map_err(|err| syn::Error::new(path_lit.span(), format!("Invalid path: {err}")))?;
	let image = ImageReader::open(&path)
		.map_err(|err| syn::Error::new(path_lit.span(), format!("Failed to open image {path:?}: {err}")))?
		.decode()
		.map_err(|err| syn::Error::new(path_lit.span(), format!("Failed to decode image {path:?}: {err}")))?;
	let path = path
		.canonicalize()
		.ok()
		.and_then(|path| path.to_str().map(String::from))
		.unwrap_or_else(|| path_lit.value());

	// convert input image to vec of colors
	let image: RgbaImage = image.into_rgba8();
	let color_ty = quote!(<#ty as ::embedded_sprites::private::Image>::Color);
	let (colors, transparency) = pixels_to_rgb888(image.pixels())?;
	let color_array = quote!([#(#colors),*]);

	// this is a transparency array which represents each bit as a byte
	let tmap_byte_array: TokenStream2 = quote!([#(#transparency),*]);
	// size of an equivalent array which compresses 8bits into a single byte
	let tmap_bit_array_len = transparency.len().div_ceil(8);

	let width = image.width() as u16;
	let height = image.height() as u16;

	let output = quote! {
		#(#attrs)* #vis #const_token #ident #colon_token #ty #eq_token {
			// include the bytes so that the compiler knows to recompile when the
			// image file changes
			const _: &[u8] = ::core::include_bytes!(#path);
			const IMAGE_SIZE: usize = (#width * #height) as usize;

			const COLOR_BYTE_ARRAY: [u8; IMAGE_SIZE * 3] = #color_array;
			const COLOR_ARRAY: [#color_ty; IMAGE_SIZE] = {
				let mut colors = [#color_ty::new(0, 0, 0); IMAGE_SIZE];
				let mut idx = 0;

				// const loop
				while idx < IMAGE_SIZE {
					let base = idx * 3;
					let (r, g, b) = ::embedded_sprites::private::convert_from_bgr888::<#color_ty>(
						COLOR_BYTE_ARRAY[base], COLOR_BYTE_ARRAY[base + 1], COLOR_BYTE_ARRAY[base + 2]
					);

					colors[idx] = #color_ty::new(r, g, b);
					idx += 1;
				}
				colors
			};

			const TRANSPARENCY_BYTE_ARRAY: [u8; IMAGE_SIZE] = #tmap_byte_array;
			const TRANSPARENCY_MAP: [u8; #tmap_bit_array_len] = {
				let mut TMAP = [0u8; #tmap_bit_array_len];
				let mut i = 0;
				let mut j = 7;
				let mut n = 0;

				// const loop
				while n < IMAGE_SIZE {
					TMAP[i] |= (TRANSPARENCY_BYTE_ARRAY[n] & 1 ) << j;
					if j == 0 {
						j = 7;
						i += 1;
					} else {
						j -= 1;
					}
					n += 1;
				}
				TMAP
			};

			match ::embedded_sprites::image::Image::<'static, #color_ty>::new(
				&COLOR_ARRAY, &TRANSPARENCY_MAP, #width, #height
			) {
				::core::result::Result::Ok(img) => img,
				_ => panic!("Failed to construct image")
			}
		}
		#semi_token
	};

	Ok(output)
}

/// Utility macro to construct a const [`Image`](embedded_graphics::image::Image) at compile time from a image file.
///
/// Every image formats supported by the [image crate](https://crates.io/crates/image) can be used.
/// The image will be automatically be converted to the requested pixelcolor.
/// Current only rgb pixelcolors are supported.
///
/// ```ignore
/// use embedded_sprites::{image::Image, include_image};
/// use embedded_graphics::pixelcolor::Bgr565;
/// #[include_image]
/// const IMAGE: Image<Bgr565> = "img/grass.png";
/// ```
#[proc_macro_attribute]
pub fn include_image(_attr: TokenStream, item: TokenStream) -> TokenStream {
	expand(parse_macro_input!(item))
		.unwrap_or_else(|err| err.into_compile_error())
		.into()
}
