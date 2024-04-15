# TCP 突然事实上断开连接的 debug 猜想

以下是一次断连的tcpdump信息。

```log
13:29:43.423297 IP 192.168.31.204.54680 > laios54-manjaro.20271: Flags [P.], seq 55423:55522, ack 167, win 5594, length 99
13:29:43.423392 IP laios54-manjaro.20271 > 192.168.31.204.54680: Flags [.], ack 55522, win 63360, length 0
13:29:43.463427 IP 192.168.31.204.54680 > laios54-manjaro.20271: Flags [P.], seq 55522:55588, ack 167, win 5594, length 66
13:29:43.463525 IP laios54-manjaro.20271 > 192.168.31.204.54680: Flags [.], ack 55588, win 63360, length 0
13:29:43.513315 IP 192.168.31.204.54680 > laios54-manjaro.20271: Flags [P.], seq 55588:55687, ack 167, win 5594, length 99
13:29:43.513419 IP laios54-manjaro.20271 > 192.168.31.204.54680: Flags [.], ack 55687, win 63360, length 0
13:29:58.797195 IP laios54-manjaro.20271 > 192.168.31.204.54680: Flags [F.], seq 167, ack 55687, win 63360, length 0
13:29:58.813116 IP 192.168.31.204.54680 > laios54-manjaro.20271: Flags [.], ack 168, win 5593, length 0
13:30:05.673426 IP 192.168.31.204.54680 > laios54-manjaro.20271: Flags [P.], seq 55687:55786, ack 168, win 5593, length 99
13:30:05.673450 IP laios54-manjaro.20271 > 192.168.31.204.54680: Flags [R], seq 3068568160, win 0, length 0
```

可以看出，虽然server无法再接收到esp发出的tcp packet，但当server发出F packet的时候esp又能很快发送ack。我怀疑是某一个需要发送到esp的ack丢失了导致esp一直在等待该ack到达后才愿意再发新的数据。

这个猜想还需要验证。目前初步想法是看看能不能稳定复现这种情况。

但一般来说，如果esp没收到ack,它在buffer满的时候也应该把数据发出。

```log
13:40:20.210400 IP 192.168.31.204.64939 > laios54-manjaro.20271: Flags [P.], seq 29371:29435, ack 167, win 5594, length 64
13:40:20.210475 IP laios54-manjaro.20271 > 192.168.31.204.64939: Flags [.], ack 29435, win 64072, length 0
13:40:20.238635 IP 192.168.31.204.64939 > laios54-manjaro.20271: Flags [P.], seq 29435:29467, ack 167, win 5594, length 32
13:40:20.238665 IP laios54-manjaro.20271 > 192.168.31.204.64939: Flags [.], ack 29467, win 64072, length 0
13:40:20.268678 IP 192.168.31.204.64939 > laios54-manjaro.20271: Flags [P.], seq 29467:29499, ack 167, win 5594, length 32
13:40:20.268732 IP laios54-manjaro.20271 > 192.168.31.204.64939: Flags [.], ack 29499, win 64072, length 0
13:40:21.100048 IP 192.168.31.204.64939 > laios54-manjaro.20271: Flags [P.], seq 29563:31003, ack 167, win 5594, length 1440
13:40:21.100074 IP laios54-manjaro.20271 > 192.168.31.204.64939: Flags [.], ack 29499, win 64072, length 0
13:40:24.690373 IP 192.168.31.204.64939 > laios54-manjaro.20271: Flags [P.], seq 29499:29563, ack 167, win 5594, length 64
13:40:24.690395 IP laios54-manjaro.20271 > 192.168.31.204.64939: Flags [.], ack 31003, win 62712, length 0
13:40:24.718659 IP 192.168.31.204.64939 > laios54-manjaro.20271: Flags [P.], seq 31003:32443, ack 167, win 5594, length 1440
13:40:24.718660 IP 192.168.31.204.64939 > laios54-manjaro.20271: Flags [P.], seq 32443:33883, ack 167, win 5594, length 1440
13:40:24.718672 IP laios54-manjaro.20271 > 192.168.31.204.64939: Flags [.], ack 32443, win 62712, length 0
13:40:24.718681 IP laios54-manjaro.20271 > 192.168.31.204.64939: Flags [.], ack 33883, win 61272, length 0
13:40:24.740033 IP 192.168.31.204.64939 > laios54-manjaro.20271: Flags [P.], seq 33883:35259, ack 167, win 5594, length 1376
13:40:24.740041 IP laios54-manjaro.20271 > 192.168.31.204.64939: Flags [.], ack 35259, win 61920, length 0
13:40:24.759446 IP 192.168.31.204.64939 > laios54-manjaro.20271: Flags [.], seq 35259:36699, ack 167, win 5594, length 1440
13:40:24.759464 IP laios54-manjaro.20271 > 192.168.31.204.64939: Flags [.], ack 36699, win 61920, length 0
13:40:24.778589 IP 192.168.31.204.64939 > laios54-manjaro.20271: Flags [P.], seq 36699:36877, ack 167, win 5594, length 178
13:40:24.778605 IP laios54-manjaro.20271 > 192.168.31.204.64939: Flags [.], ack 36877, win 63360, length 0
13:40:24.788873 IP 192.168.31.204.64939 > laios54-manjaro.20271: Flags [P.], seq 36877:36943, ack 167, win 5594, length 66
13:40:24.788885 IP laios54-manjaro.20271 > 192.168.31.204.64939: Flags [.], ack 36943, win 63360, length 0
13:40:24.808606 IP 192.168.31.204.64939 > laios54-manjaro.20271: Flags [P.], seq 36943:36976, ack 167, win 5594, length 33
```

```log
13:47:13.161922 IP 192.168.31.204.64843 > laios54-manjaro.20271: Flags [P.], seq 63739:64234, ack 167, win 5594, length 495
13:47:13.161941 IP laios54-manjaro.20271 > 192.168.31.204.64843: Flags [.], ack 64234, win 63289, length 0
13:47:15.214087 IP 192.168.31.204.64843 > laios54-manjaro.20271: Flags [P.], seq 64234:65674, ack 167, win 5594, length 1440
13:47:15.214111 IP laios54-manjaro.20271 > 192.168.31.204.64843: Flags [.], ack 68554, win 58969, length 0
13:47:15.282435 IP 192.168.31.204.64843 > laios54-manjaro.20271: Flags [P.], seq 68554:69994, ack 167, win 5594, length 1440
13:47:15.282449 IP laios54-manjaro.20271 > 192.168.31.204.64843: Flags [.], ack 69994, win 65535, length 0
13:47:30.835614 IP laios54-manjaro.20271 > 192.168.31.204.64843: Flags [F.], seq 167, ack 69994, win 65535, length 0
13:47:30.851652 IP 192.168.31.204.64843 > laios54-manjaro.20271: Flags [.], ack 168, win 5593, length 0
13:47:37.642081 IP 192.168.31.204.64843 > laios54-manjaro.20271: Flags [P.], seq 69994:71434, ack 168, win 5593, length 1440
13:47:37.642108 IP laios54-manjaro.20271 > 192.168.31.204.64843: Flags [R], seq 3321893049, win 0, length 0
```

值得注意的是，在上面的log中，esp很快地响应了F packet,但在7秒后又尝试继续推送旧数据。推送的seq从上一个ack开始，说明esp32接收到了ack，但在发送过程中受到了阻碍。

```log
13:52:27.497668 IP laios54-manjaro.20271 > 192.168.31.204.57443: Flags [.], ack 97420, win 65535, length 0
13:52:28.084207 IP 192.168.31.204.57443 > laios54-manjaro.20271: Flags [P.], seq 98860:100300, ack 167, win 5594, length 1440
13:52:28.084228 IP laios54-manjaro.20271 > 192.168.31.204.57443: Flags [.], ack 97420, win 65535, length 0
13:52:37.614385 IP 192.168.31.204.57443 > laios54-manjaro.20271: Flags [P.], seq 97420:98860, ack 167, win 5594, length 1440
13:52:37.614404 IP laios54-manjaro.20271 > 192.168.31.204.57443: Flags [.], ack 100300, win 65535, length 0
13:52:37.874477 IP 192.168.31.204.57443 > laios54-manjaro.20271: Flags [.], seq 101278:102718, ack 167, win 5594, length 1440
13:52:37.874488 IP laios54-manjaro.20271 > 192.168.31.204.57443: Flags [.], ack 100300, win 65535, length 0
13:52:52.847055 IP laios54-manjaro.20271 > 192.168.31.204.57443: Flags [F.], seq 167, ack 100300, win 65535, length 0
13:52:52.884287 IP 192.168.31.204.57443 > laios54-manjaro.20271: Flags [.], ack 168, win 5593, length 0
13:53:24.164800 IP 192.168.31.204.57443 > laios54-manjaro.20271: Flags [P.], seq 100300:101278, ack 168, win 5593, length 978
13:53:24.164826 IP laios54-manjaro.20271 > 192.168.31.204.57443: Flags [R], seq 4269714103, win 0, length 0
```

无论如何，看起来esp总是能够较快地（372ms）响应F packet,在过了很久以后又尝试从最后一次ack的地方推送数据。我认为可能和wifi的一些机制有关。

也许我应该试试利用我的电脑作为AP，以防止路由器可能存在的问题。

```log
13:58:21.945338 IP laios54-manjaro.20271 > 192.168.31.204.64179: Flags [.], ack 248671, win 65535, length 0
13:58:23.275646 IP 192.168.31.204.64179 > laios54-manjaro.20271: Flags [P.], seq 248671:249133, ack 167, win 5594, length 462
13:58:23.275667 IP laios54-manjaro.20271 > 192.168.31.204.64179: Flags [.], ack 249133, win 65535, length 0
13:58:38.801473 IP laios54-manjaro.20271 > 192.168.31.204.64179: Flags [F.], seq 167, ack 249133, win 65535, length 0
13:58:38.834540 IP 192.168.31.204.64179 > laios54-manjaro.20271: Flags [.], ack 168, win 5593, length 0
13:58:45.694919 IP 192.168.31.204.64179 > laios54-manjaro.20271: Flags [P.], seq 249133:250573, ack 168, win 5593, length 1440
13:58:45.694943 IP laios54-manjaro.20271 > 192.168.31.204.64179: Flags [R], seq 3004020368, win 0, length 0
```

同样，在上面的log中，esp较快地响应了F packet,但在之后较久的时间后又尝试push数据。无论如何，至少可以确认电脑没有出现问题。

目前说来，esp会在一分钟以内出现这个问题。收集一下log确定一下频率。如果时间上差得不大，可能在软件方面存在问题。如果有几秒有几十秒的，更可能是wifi问题。

无论如何，还是应该确认利用tcp socket高速发送消息还会不会出现问题如果确实是和wifi相关的问题的话，应该能够稳定复现问题。

```log
14:16:28.885582 IP laios54-manjaro.20271 > 192.168.31.204.57128: Flags [.], ack 66082, win 65535, length 0
14:16:29.737301 IP 192.168.31.204.57128 > laios54-manjaro.20271: Flags [P.], seq 66214:67654, ack 167, win 5594, length 1440
14:16:29.737321 IP laios54-manjaro.20271 > 192.168.31.204.57128: Flags [.], ack 66082, win 65535, length 0
14:16:43.891940 IP laios54-manjaro.20271 > 192.168.31.204.57128: Flags [F.], seq 167, ack 66082, win 65535, length 0
14:16:44.152985 IP laios54-manjaro.20271 > 192.168.31.204.57128: Flags [F.], seq 167, ack 66082, win 65535, length 0
14:16:44.175303 IP 192.168.31.204.57128 > laios54-manjaro.20271: Flags [.], ack 168, win 5593, length 0
14:17:15.205762 IP 192.168.31.204.57128 > laios54-manjaro.20271: Flags [P.], seq 66082:66214, ack 168, win 5593, length 132
14:17:15.205788 IP laios54-manjaro.20271 > 192.168.31.204.57128: Flags [R], seq 2759637750, win 0, length 0
```

以上的log可以看出来有时候esp也没收到F packet,但收到后就会很快响应。其背后原因可能是某段时间wifi对于esp来说不可用，而tcp的等待重传机制放大了这种不可用性。

目前来看最有可能的问题：
1）esp的wifi连接弱。通过带天线的开发板可以验证。
2）tcp的问题放大了网络问题。换用udp试试。

不太可能的问题：
1）ws客户端的问题。
2）线程饿死。需要进一步确认esp-idf wifi模块的设计。

因为有可能多种问题混杂，尽量从好确认的问题开始确认。逐步排查。

可以试试以下改进手段：

1）使用蓝牙
2）使用udp
