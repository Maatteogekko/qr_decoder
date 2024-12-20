# qr_decoder

This is a small utility server that scans a PDF or image and returns a list of barcode values found in the file.

## Setup

In order to make this program work, the `pdfium` dynamic library must be present in the project root folder.
See [here](https://docs.rs/pdfium-render/0.8.25/pdfium_render/#binding-to-pdfium) for more info.

Find the correct image for your architecture [here](https://github.com/bblanchon/pdfium-binaries/releases), download and extract it. Then copy the file inside **lib** at the root of the project. Adjust the `COPY` command at the end of the [Dockerfile](Dockerfile) if needed.

## Use

Look at [api.rest](api.rest) for an example of how to invoke the service.
