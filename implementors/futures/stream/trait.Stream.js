(function() {var implementors = {};
implementors["futures"] = [];
implementors["tokio_io"] = [{text:"impl&lt;T, U&gt; <a class=\"trait\" href=\"futures/stream/trait.Stream.html\" title=\"trait futures::stream::Stream\">Stream</a> for <a class=\"struct\" href=\"tokio_io/codec/struct.Framed.html\" title=\"struct tokio_io::codec::Framed\">Framed</a>&lt;T, U&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;T: <a class=\"trait\" href=\"tokio_io/trait.AsyncRead.html\" title=\"trait tokio_io::AsyncRead\">AsyncRead</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;U: <a class=\"trait\" href=\"tokio_io/codec/trait.Decoder.html\" title=\"trait tokio_io::codec::Decoder\">Decoder</a>,&nbsp;</span>",synthetic:false,types:["tokio_io::framed::Framed"]},{text:"impl&lt;T, D&gt; <a class=\"trait\" href=\"futures/stream/trait.Stream.html\" title=\"trait futures::stream::Stream\">Stream</a> for <a class=\"struct\" href=\"tokio_io/codec/struct.FramedRead.html\" title=\"struct tokio_io::codec::FramedRead\">FramedRead</a>&lt;T, D&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;T: <a class=\"trait\" href=\"tokio_io/trait.AsyncRead.html\" title=\"trait tokio_io::AsyncRead\">AsyncRead</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;D: <a class=\"trait\" href=\"tokio_io/codec/trait.Decoder.html\" title=\"trait tokio_io::codec::Decoder\">Decoder</a>,&nbsp;</span>",synthetic:false,types:["tokio_io::framed_read::FramedRead"]},{text:"impl&lt;T, D&gt; <a class=\"trait\" href=\"futures/stream/trait.Stream.html\" title=\"trait futures::stream::Stream\">Stream</a> for <a class=\"struct\" href=\"tokio_io/codec/struct.FramedWrite.html\" title=\"struct tokio_io::codec::FramedWrite\">FramedWrite</a>&lt;T, D&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;T: <a class=\"trait\" href=\"futures/stream/trait.Stream.html\" title=\"trait futures::stream::Stream\">Stream</a>,&nbsp;</span>",synthetic:false,types:["tokio_io::framed_write::FramedWrite"]},{text:"impl&lt;T:&nbsp;<a class=\"trait\" href=\"tokio_io/trait.AsyncRead.html\" title=\"trait tokio_io::AsyncRead\">AsyncRead</a>, B:&nbsp;<a class=\"trait\" href=\"bytes/buf/into_buf/trait.IntoBuf.html\" title=\"trait bytes::buf::into_buf::IntoBuf\">IntoBuf</a>&gt; <a class=\"trait\" href=\"futures/stream/trait.Stream.html\" title=\"trait futures::stream::Stream\">Stream</a> for <a class=\"struct\" href=\"tokio_io/codec/length_delimited/struct.Framed.html\" title=\"struct tokio_io::codec::length_delimited::Framed\">Framed</a>&lt;T, B&gt;",synthetic:false,types:["tokio_io::length_delimited::Framed"]},{text:"impl&lt;T:&nbsp;<a class=\"trait\" href=\"tokio_io/trait.AsyncRead.html\" title=\"trait tokio_io::AsyncRead\">AsyncRead</a>&gt; <a class=\"trait\" href=\"futures/stream/trait.Stream.html\" title=\"trait futures::stream::Stream\">Stream</a> for <a class=\"struct\" href=\"tokio_io/codec/length_delimited/struct.FramedRead.html\" title=\"struct tokio_io::codec::length_delimited::FramedRead\">FramedRead</a>&lt;T&gt;",synthetic:false,types:["tokio_io::length_delimited::FramedRead"]},{text:"impl&lt;T:&nbsp;<a class=\"trait\" href=\"futures/stream/trait.Stream.html\" title=\"trait futures::stream::Stream\">Stream</a>, B:&nbsp;<a class=\"trait\" href=\"bytes/buf/into_buf/trait.IntoBuf.html\" title=\"trait bytes::buf::into_buf::IntoBuf\">IntoBuf</a>&gt; <a class=\"trait\" href=\"futures/stream/trait.Stream.html\" title=\"trait futures::stream::Stream\">Stream</a> for <a class=\"struct\" href=\"tokio_io/codec/length_delimited/struct.FramedWrite.html\" title=\"struct tokio_io::codec::length_delimited::FramedWrite\">FramedWrite</a>&lt;T, B&gt;",synthetic:false,types:["tokio_io::length_delimited::FramedWrite"]},{text:"impl&lt;A&gt; <a class=\"trait\" href=\"futures/stream/trait.Stream.html\" title=\"trait futures::stream::Stream\">Stream</a> for <a class=\"struct\" href=\"tokio_io/io/struct.Lines.html\" title=\"struct tokio_io::io::Lines\">Lines</a>&lt;A&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;A: <a class=\"trait\" href=\"tokio_io/trait.AsyncRead.html\" title=\"trait tokio_io::AsyncRead\">AsyncRead</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/std/io/trait.BufRead.html\" title=\"trait std::io::BufRead\">BufRead</a>,&nbsp;</span>",synthetic:false,types:["tokio_io::lines::Lines"]},];
implementors["tokio_tcp"] = [{text:"impl <a class=\"trait\" href=\"futures/stream/trait.Stream.html\" title=\"trait futures::stream::Stream\">Stream</a> for <a class=\"struct\" href=\"tokio_tcp/struct.Incoming.html\" title=\"struct tokio_tcp::Incoming\">Incoming</a>",synthetic:false,types:["tokio_tcp::incoming::Incoming"]},];
implementors["tokio_timer"] = [{text:"impl <a class=\"trait\" href=\"futures/stream/trait.Stream.html\" title=\"trait futures::stream::Stream\">Stream</a> for <a class=\"struct\" href=\"tokio_timer/struct.Interval.html\" title=\"struct tokio_timer::Interval\">Interval</a>",synthetic:false,types:["tokio_timer::interval::Interval"]},];
implementors["tokio_udp"] = [{text:"impl&lt;C:&nbsp;<a class=\"trait\" href=\"tokio_io/codec/decoder/trait.Decoder.html\" title=\"trait tokio_io::codec::decoder::Decoder\">Decoder</a>&gt; <a class=\"trait\" href=\"futures/stream/trait.Stream.html\" title=\"trait futures::stream::Stream\">Stream</a> for <a class=\"struct\" href=\"tokio_udp/struct.UdpFramed.html\" title=\"struct tokio_udp::UdpFramed\">UdpFramed</a>&lt;C&gt;",synthetic:false,types:["tokio_udp::frame::UdpFramed"]},];

            if (window.register_implementors) {
                window.register_implementors(implementors);
            } else {
                window.pending_implementors = implementors;
            }
        
})()