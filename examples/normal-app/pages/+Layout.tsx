import { Fragment, ReactNode } from "react";
import "./styles.css";

export function Layout({ children }: { children: ReactNode }) {
  return <Fragment>{children}</Fragment>;
}
