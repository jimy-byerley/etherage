#!/usr/bin/python3

import sys
import numpy as np
import pyqtgraph as pg
from eve.apputils import *
from PyQt5.QtGui import QColor

def style(color):
	return dict(
		pen = (color[0], color[1], color[2], 150),
		symbol = 'o',
		symbolSize = 3,
		symbolBrush = color,
		symbolPen = None,
		)

@qtschedule
def show():

	source = sys.argv[1]
	period = 2e-3

	record = []
	for line in open(source, 'r').readlines():
		groups = line.split()
		frame = []
		for group in groups:
			try:	frame.append(int(group))
			except ValueError:  break
		else:
			record.append(frame)
	record = np.array(record)
		
	pg.setConfigOptions(antialias=True)
	win = qtwindow(pg.GraphicsLayoutWidget(title=source))
	win.resize(1000, 300)
	fig = win.addPlot()
	
	time = np.arange(0, len(record)) * period

	colorcycle = [(100, 255, 100), (100, 100, 255), (100, 200, 200), (200, 200, 200), (200, 100, 255), (200, 255, 100)]
	reference = record[:,0]
	for i, slave in enumerate(record.transpose()):
		fig.plot(reference * 1e-9, slave - reference, label=f'slave {i}', **style(colorcycle[i%len(colorcycle)]))

qtmain()
