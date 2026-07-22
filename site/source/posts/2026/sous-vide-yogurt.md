---
title: Kinetic stabilization's effect on yogurt
date: 2026-05-24
tags: cooking
---

Yogurt is a neat interplay of biology, chemistry, and physics. Physics is usually disregarded, but I've found it to be a big contributor to the resulting texture of the yogurt.

Unsolved problems haunt you in a good way. Long ago, I had tried to replicate the velvety texture of Straus's European-style yogurt at home. But my results were grainy and weepy. Dredging through the recesses of our kitchen to prepare for a move, I found our lonely sous vide. It stirred a latent curiosity: should I try with this?

> [Straus yogurt] is slowly cultured and vat-set. Unlike other yogurts, which are incubated inside their plastic cups, we incubate our yogurt in stainless-steel vats and fill our recyclable plastic containers with cooled yogurt.

Presumably they use glycol-jacketed vats to heat, cool, and ferment the milk in bulk. My intuition is that by fermenting (and cooling!) with thermal envelopes you effectively eliminate kinetic disruptions (sloshing, convective heating, shearing). In my research, no yogurt recipe mentioned kinetic stability, but I believe it's a huge contributor to the resulting mouthfeel of the yogurt.

I don't understand all the forces at play, but in brief, we're transforming liquid (milk) into a gel by neutralizing the electrostatic charge of the proteins so they can bind into a three-dimensional mesh. As the milk transitions to a gel, those delicate connections can be irreparably ruptured by movement. Maintaining a completely stationary vessel throughout fermentation and cooling preserves the delicate structural integrity of the developing gel.

The graph below shows the general approach, where time and temperature are not as critical as minimizing agitation.

<svg viewBox="0 0 600 290" class="chart" role="img" aria-label="Sous vide yogurt temperature profile: heat to 185°F to denature, cool to 110°F, inoculate, ferment for a variable duration, then refrigerate to 38°F. Once inoculated, the milk must not be agitated.">
  <!-- axes -->
  <line x1="60" y1="30"  x2="60"  y2="260" stroke="currentColor" stroke-width="1"/>
  <line x1="60" y1="260" x2="580" y2="260" stroke="currentColor" stroke-width="1"/>

  <!-- no-agitation zone: from inoculation through cool-down, the protein
       gel is forming and any movement ruptures the matrix. -->
  <path d="M 220,90 L 220,85 L 570,85 L 570,90"
        fill="none" stroke="currentColor" stroke-width="0.75" opacity="0.5"/>
  <text x="395" y="78" text-anchor="middle" font-size="11" fill="currentColor" opacity="0.7">no agitation</text>

  <!-- temp curve: smooth arc up to denature, down to ferment, dashed for the
       variable-length ferment, then a smooth descent to fridge temperature. -->
  <path fill="none" stroke="#c0392b" stroke-width="2"
        d="M 60,204
           C 90,204 108,50 128,50
           C 148,50 166,152 196,152
           L 360,152"/>
  <path fill="none" stroke="#c0392b" stroke-width="2" stroke-dasharray="5,4"
        d="M 360,152 L 490,152"/>
  <path fill="none" stroke="#c0392b" stroke-width="2"
        d="M 490,152 C 520,152 540,253 570,253"/>

  <!-- inoculate: starter goes in once the curve has cooled to 110°F. Dot sits
       a bit right of the elbow so the smooth curve→flat join stays visible. -->
  <circle cx="220" cy="152" r="3.5" fill="#fff" stroke="#c0392b" stroke-width="1.5"/>

  <!-- point labels -->
  <text x="128" y="40"  text-anchor="middle" font-size="11" fill="currentColor">denature</text>
  <text x="220" y="167" text-anchor="middle" font-size="11" fill="currentColor">inoculate</text>
  <text x="290" y="145" text-anchor="middle" font-size="11" fill="currentColor">ferment</text>
  <text x="425" y="167" text-anchor="middle" font-size="11" fill="currentColor">variable hours</text>
  <text x="510" y="230" text-anchor="middle" font-size="11" fill="currentColor">chill</text>

  <!-- y ticks -->
  <text x="54" y="34"  text-anchor="end" font-size="11" fill="currentColor">200°F</text>
  <text x="54" y="115" text-anchor="end" font-size="11" fill="currentColor">140°F</text>
  <text x="54" y="196" text-anchor="end" font-size="11" fill="currentColor">80°F</text>
  <text x="54" y="256" text-anchor="end" font-size="11" fill="currentColor">38°F</text>

</svg>

Sous vide allows you to uniformly heat, cool, and ferment the milk with zero physical disturbance. In all of the subsequent batches I inoculate with 5% by weight. The only variance is time and temperature.

![Yogurt fermenting sous vide](yogurt-sous-vide-setup "Jank vat-style home setup")

## Batch 1

* 20-minute denature at 185°F
* 8 hours at 110°F

Beginner's luck brought me a custard-like mass with very low whey separation (syneresis).

{{< video name="yogurt-texture-test" caption="#1: remarkably good custard" >}}

## Batch 2

* 5-minute denature at 195°F
* 12 hours at 89°F

The goal of this batch was to encourage gel strength through aggressive denaturing and a more gradual drop in pH, allowing the proteins plenty of time to organize. Unfortunately, at that low temperature, the cultures threw off a bunch of long-chain sugars (exopolysaccharides) that resulted in an off-putting ropey texture.

{{< video name="yogurt-195-88" caption="#2: off-putting but still edible" >}}
