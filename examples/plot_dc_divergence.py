import sys
import matplotlib
from matplotlib import pyplot as plt

source = sys.argv[1]

record = []
for line in open(source, 'r').readlines():
	groups = line.split()
	frame = []
	for group in groups:
		try:	frame.append(int(group))
		except ValueError:  break
	else:
		record.append(frame)

matplotlib.use('qtagg')
plt.plot(record)
plt.show(block=True)
