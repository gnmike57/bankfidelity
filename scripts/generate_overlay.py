import sys
import pymupdf
import numpy as np
from PIL import Image

def generate_overlay(original_pdf, modified_pdf, output_png):
    with pymupdf.open(original_pdf) as doc1, pymupdf.open(modified_pdf) as doc2:
        page1 = doc1[0]
        page2 = doc2[0]
        
        pix1 = page1.get_pixmap(dpi=300)
        pix2 = page2.get_pixmap(dpi=300)
        
        img1 = Image.frombytes("RGB", [pix1.width, pix1.height], pix1.samples)
        img2 = Image.frombytes("RGB", [pix2.width, pix2.height], pix2.samples)
        
        # Ensure same size
        if img1.size != img2.size:
            print("Images are not the same size!")
            sys.exit(1)
            
        arr1 = np.array(img1)
        arr2 = np.array(img2)
        
        # Calculate diff
        diff = np.abs(arr1.astype(int) - arr2.astype(int)).astype(np.uint8)
        
        # Create overlay: original in grayscale, differences highlighted in red
        gray1 = np.mean(arr1, axis=2).astype(np.uint8)
        overlay = np.stack((gray1, gray1, gray1), axis=2)
        
        # Where diff is non-zero, make it red
        mask = np.any(diff > 0, axis=2)
        overlay[mask] = [255, 0, 0]
        
        out_img = Image.fromarray(overlay)
        out_img.save(output_png)
        print(f"Overlay saved to {output_png}")

if __name__ == "__main__":
    if len(sys.argv) != 4:
        print("Usage: python generate_overlay.py <original.pdf> <modified.pdf> <output.png>")
        sys.exit(1)
    
    generate_overlay(sys.argv[1], sys.argv[2], sys.argv[3])
