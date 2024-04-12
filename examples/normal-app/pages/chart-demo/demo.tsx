import { EChart } from "@kbox-labs/react-echarts";
import useWebSocket, { ReadyState } from "react-use-websocket";
import { push_data_url } from "../../src/config";
import { useEffect, useReducer, useRef, useState } from "react";

export function Demo() {
  useEcgData();

  const [data, _] = useEcgData();

  const xLabels = useRef([] as number[]);
  if (xLabels.current.length == 0) {
    xLabels.current = new Array(DATA_ARR_LEN).fill(0) as number[];
    xLabels.current = xLabels.current.map((_value, index) => {
      return index;
    });
  }

  return (
    <main>
      <div className="w-full h-[600px]">
        <EChart
          className="h-full"
          renderer={"canvas"}
          onClick={() => console.log("clicked!")}
          // dataset={{
          //   source: data,
          // }}
          xAxis={{
            data: xLabels.current,
          }}
          yAxis={{
            type: "value",
            boundaryGap: ['20%', '20%'],
            // scale: true
          }}
          series={[
            {
              type: "line",
              smooth: true,
              data: data,
            },
          ]}
        />
      </div>
    </main>
  );
}

function useLogMsg(lastJsonMessage: any, readyState: ReadyState) {
  const debug_counter = useRef(0);
  useEffect(() => {
    debug_counter.current++;
    if (debug_counter.current >= 180) {
      console.debug(lastJsonMessage, readyState);
      debug_counter.current = 0;
    }
  }, [lastJsonMessage, readyState]);
}

const DATA_ARR_LEN = 60 * 5;
const ECG_LINE_WIDTH = 10;

type EcgData = [number, { id: number; value: number }];

function useEcgData(): [(number | null)[], number] {
  const { lastJsonMessage, readyState } = useWebSocket<EcgData>(push_data_url, {
    heartbeat: {
      message: "ping",
      returnMessage: "pong",
      timeout: 3000,
      interval: 1000,
    },
  });

  useLogMsg(lastJsonMessage, readyState);

  const data_arr = useRef(null as unknown as (number | null)[]);

  function clearDataArr() {
    data_arr.current = new Array(DATA_ARR_LEN).fill(null) as (number | null)[];
  }

  function clearRange(start: number, end: number) {
    for (let i = start; i < end; i++) {
      data_arr.current[i] = null;
    }
  }

  function updateDataArr(id: number, value: number) {
    let left_index = id % DATA_ARR_LEN;
    let right_index = (left_index + ECG_LINE_WIDTH) % DATA_ARR_LEN;

    if (right_index > left_index) {
      clearRange(left_index + 1, right_index);
    } else {
      clearRange(left_index, data_arr.current.length);
      clearRange(0, right_index);
    }

    data_arr.current[left_index] = value;
  }

  const [nonce, increaseNonce] = useReducer((v) => v++, 0);
  const [currentDataProcessorID, setCurrentDataProcessorID] = useState(-1);

  useEffect(() => {
    if (!lastJsonMessage) return;
    if (currentDataProcessorID != lastJsonMessage[0]) {
      clearDataArr();
      setCurrentDataProcessorID(lastJsonMessage[0]);
    }

    updateDataArr(lastJsonMessage[1].id, lastJsonMessage[1].value);
    increaseNonce();
  }, [lastJsonMessage]);

  return [data_arr.current, nonce];
}
