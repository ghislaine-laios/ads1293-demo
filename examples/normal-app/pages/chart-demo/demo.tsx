import { EChart } from "@kbox-labs/react-echarts";
import useWebSocket from "react-use-websocket";
import { push_data_url } from "../../src/config";
import { useEffect } from "react";

export function Demo() {
  const { lastJsonMessage, readyState} = useWebSocket(push_data_url, {heartbeat: {
    message: "ping",
    returnMessage: "pong",
    timeout: 3000,
    interval: 1000
  }});

  useEffect(() => {
    console.debug(lastJsonMessage, readyState);
  }, [lastJsonMessage, readyState]);

  return (
    <main>
      <div className="w-full h-[600px]">
        <EChart
          className="h-full"
          renderer={"canvas"}
          onClick={() => console.log("clicked!")}
          dataset={{
            source: [
              ["2022-10-17", 300],
              ["2022-10-18", 100],
            ],
          }}
          xAxis={{
            type: "category",
          }}
          yAxis={{
            type: "value",
            boundaryGap: [0, "30%"],
          }}
          series={[
            {
              type: "line",
            },
          ]}
        />
      </div>
    </main>
  );
}
