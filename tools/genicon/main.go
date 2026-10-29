// 生成 WeKnora Desktop 应用图标(1024x1024 PNG)
package main

import (
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"math"
	"os"
)

const S = 1024

func lerp(a, b float64, t float64) float64 { return a + (b-a)*t }

func main() {
	img := image.NewRGBA(image.Rect(0, 0, S, S))
	draw.Draw(img, img.Bounds(), &image.Uniform{color.RGBA{0, 0, 0, 0}}, image.Point{}, draw.Src)

	// 圆角底板 + 对角渐变(靛蓝 -> 青色)
	radius := 200.0
	c1 := [3]float64{0x34 / 255.0, 0x54 / 255.0, 0xd4 / 255.0} // indigo
	c2 := [3]float64{0x16 / 255.0, 0xc8 / 255.0, 0xe8 / 255.0} // cyan
	for y := 0; y < S; y++ {
		for x := 0; x < S; x++ {
			if inRoundedRect(float64(x)+0.5, float64(y)+0.5, radius) {
				t := (float64(x) + float64(y)) / (2 * S)
				r := uint8(lerp(c1[0], c2[0], t) * 255)
				g := uint8(lerp(c1[1], c2[1], t) * 255)
				b := uint8(lerp(c1[2], c2[2], t) * 255)
				img.Set(x, y, color.RGBA{r, g, b, 255})
			}
		}
	}

	// 中心图形:三层知识堆叠(圆角横条,逐渐变窄)+ 顶部知识星
	white := color.RGBA{255, 255, 255, 235}
	soft := color.RGBA{255, 255, 255, 150}
	drawRoundedBar(img, 512, 660, 240, 44, 22, soft)
	drawRoundedBar(img, 512, 560, 330, 52, 26, soft)
	drawRoundedBar(img, 512, 452, 420, 62, 31, white)
	drawStar(img, 512, 285, 88, white)

	f, err := os.Create("icon.png")
	if err != nil {
		panic(err)
	}
	defer f.Close()
	if err := png.Encode(f, img); err != nil {
		panic(err)
	}
}

func inRoundedRect(x, y, r float64) bool {
	m := r
	if x < m {
		if y < m {
			return (x-m)*(x-m)+(y-m)*(y-m) <= r*r
		}
		if y > S-m {
			return (x-m)*(x-m)+(y-S+m)*(y-S+m) <= r*r
		}
		return x >= 0
	}
	if x > S-m {
		if y < m {
			return (x-S+m)*(x-S+m)+(y-m)*(y-m) <= r*r
		}
		if y > S-m {
			return (x-S+m)*(x-S+m)+(y-S+m)*(y-S+m) <= r*r
		}
		return x <= S
	}
	return y >= 0 && y <= S
}

func drawRoundedBar(img *image.RGBA, cx, cy, w, h, r float64, c color.RGBA) {
	for y := int(cy - h/2); y < int(cy+h/2); y++ {
		for x := int(cx - w/2); x < int(cx+w/2); x++ {
			px, py := float64(x)+0.5, float64(y)+0.5
			dx := math.Max(math.Abs(px-cx)-w/2+r, 0)
			dy := math.Max(math.Abs(py-cy)-h/2+r, 0)
			if dx*dx+dy*dy <= r*r {
				img.Set(x, y, c)
			}
		}
	}
}

func drawStar(img *image.RGBA, cx, cy, R float64, c color.RGBA) {
	// 四芒星(菱形星光)
	for y := int(cy - R); y < int(cy+R); y++ {
		for x := int(cx - R); x < int(cx+R); x++ {
			px, py := float64(x)+0.5-cx, float64(y)+0.5-cy
			d := math.Abs(px) + math.Abs(py)
			if d <= R {
				t := 1.0 - d/R*0.25
				img.Set(x, y, color.RGBA{
					uint8(float64(c.R) * t),
					uint8(float64(c.G) * t),
					uint8(float64(c.B) * t),
					c.A,
				})
			}
		}
	}
}
